use std::time::{Duration, SystemTime};
use glob::Pattern;
use crate::model::FileNode;

#[derive(Debug, Clone, PartialEq)]
pub enum SizeFilter {
    GreaterThan(u64),
    LessThan(u64),
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgeFilter {
    NewerThan(Duration), // < 7d
    OlderThan(Duration), // > 30d
}

#[derive(Debug, Clone, PartialEq)]
pub enum TagFilter {
    Specific(u8),
    Any,
    None,
}

#[derive(Debug, Clone, Default)]
pub struct FilterCriteria {
    pub raw_query: String,
    pub name_patterns: Vec<(Pattern, bool)>, // (pattern, is_negated)
    pub size_filters: Vec<SizeFilter>,
    pub age_filters: Vec<AgeFilter>,
    pub tag_filter: Option<TagFilter>,
    pub dirs_only: bool,
    pub files_only: bool,
}

impl FilterCriteria {
    pub fn parse(query: &str) -> Self {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Self::default();
        }

        let mut criteria = Self {
            raw_query: trimmed.to_string(),
            ..Default::default()
        };

        // Split by semicolon or spaces
        let tokens: Vec<&str> = trimmed
            .split(|c| c == ';' || c == ' ')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();

        for token in tokens {
            if token == "is:dir" || token == "type:dir" {
                criteria.dirs_only = true;
            } else if token == "is:file" || token == "type:file" {
                criteria.files_only = true;
            } else if let Some(tag_str) = token.strip_prefix("tag:") {
                match tag_str.to_lowercase().as_str() {
                    "1" | "red" => criteria.tag_filter = Some(TagFilter::Specific(1)),
                    "2" | "green" => criteria.tag_filter = Some(TagFilter::Specific(2)),
                    "3" | "yellow" => criteria.tag_filter = Some(TagFilter::Specific(3)),
                    "4" | "blue" => criteria.tag_filter = Some(TagFilter::Specific(4)),
                    "any" => criteria.tag_filter = Some(TagFilter::Any),
                    "none" | "0" => criteria.tag_filter = Some(TagFilter::None),
                    _ => {}
                }
            } else if token.starts_with('>') || token.starts_with('<') {
                let is_gt = token.starts_with('>');
                let val_str = &token[1..];
                // Could be size (>100M) or age (>30d)
                if val_str.ends_with('d') || val_str.ends_with('h') || val_str.ends_with('m') || val_str.ends_with('y') {
                    if let Some(dur) = parse_duration(val_str) {
                        if is_gt {
                            criteria.age_filters.push(AgeFilter::OlderThan(dur));
                        } else {
                            criteria.age_filters.push(AgeFilter::NewerThan(dur));
                        }
                        continue;
                    }
                }
                if let Some(bytes) = parse_size(val_str) {
                    if is_gt {
                        criteria.size_filters.push(SizeFilter::GreaterThan(bytes));
                    } else {
                        criteria.size_filters.push(SizeFilter::LessThan(bytes));
                    }
                }
            } else {
                // Name / glob pattern
                let (pattern_str, negated) = if let Some(p) = token.strip_prefix('!') {
                    (p, true)
                } else {
                    (token, false)
                };

                let glob_str = if !pattern_str.contains('*') && !pattern_str.contains('?') {
                    format!("*{}*", pattern_str)
                } else {
                    pattern_str.to_string()
                };

                if let Ok(pattern) = Pattern::new(&glob_str.to_lowercase()) {
                    criteria.name_patterns.push((pattern, negated));
                }
            }
        }

        criteria
    }

    pub fn matches(&self, node: &FileNode) -> bool {
        // If criteria is empty, match everything
        if self.raw_query.is_empty() {
            return true;
        }

        // Directories: we always allow them if they might contain matching children,
        // but if dirs_only is set, files are excluded.
        if self.dirs_only && !node.is_dir() {
            return false;
        }
        if self.files_only && node.is_dir() {
            return false;
        }

        // For files, evaluate all filters
        if !node.is_dir() {
            // Tag filter
            if let Some(ref tf) = self.tag_filter {
                match tf {
                    TagFilter::Specific(t) => {
                        if node.tag != Some(*t) {
                            return false;
                        }
                    }
                    TagFilter::Any => {
                        if node.tag.is_none() {
                            return false;
                        }
                    }
                    TagFilter::None => {
                        if node.tag.is_some() {
                            return false;
                        }
                    }
                }
            }

            // Size filter
            for sf in &self.size_filters {
                match sf {
                    SizeFilter::GreaterThan(sz) => {
                        if node.size <= *sz {
                            return false;
                        }
                    }
                    SizeFilter::LessThan(sz) => {
                        if node.size >= *sz {
                            return false;
                        }
                    }
                }
            }

            // Age filter
            if !self.age_filters.is_empty() {
                if let Some(mod_time) = node.modified {
                    if let Ok(elapsed) = SystemTime::now().duration_since(mod_time) {
                        for af in &self.age_filters {
                            match af {
                                AgeFilter::NewerThan(dur) => {
                                    if elapsed > *dur {
                                        return false;
                                    }
                                }
                                AgeFilter::OlderThan(dur) => {
                                    if elapsed < *dur {
                                        return false;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Name pattern filter
            let node_name_lower = node.name.to_lowercase();
            for (pattern, negated) in &self.name_patterns {
                let matched = pattern.matches(&node_name_lower);
                if *negated && matched {
                    return false;
                }
                if !*negated && !matched {
                    return false;
                }
            }
        }

        true
    }
}

fn parse_size(s: &str) -> Option<u64> {
    let s = s.trim().to_uppercase();
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    let (num_part, unit) = if let Some(stripped) = s.strip_suffix("TB") {
        (stripped, TB)
    } else if let Some(stripped) = s.strip_suffix('T') {
        (stripped, TB)
    } else if let Some(stripped) = s.strip_suffix("GB") {
        (stripped, GB)
    } else if let Some(stripped) = s.strip_suffix('G') {
        (stripped, GB)
    } else if let Some(stripped) = s.strip_suffix("MB") {
        (stripped, MB)
    } else if let Some(stripped) = s.strip_suffix('M') {
        (stripped, MB)
    } else if let Some(stripped) = s.strip_suffix("KB") {
        (stripped, KB)
    } else if let Some(stripped) = s.strip_suffix('K') {
        (stripped, KB)
    } else if let Some(stripped) = s.strip_suffix('B') {
        (stripped, 1u64)
    } else {
        (s.as_str(), 1u64)
    };

    let num: f64 = num_part.trim().parse().ok()?;
    Some((num * unit as f64) as u64)
}

fn parse_duration(s: &str) -> Option<Duration> {
    let s = s.trim().to_lowercase();
    let (num_part, multiplier_secs) = if let Some(stripped) = s.strip_suffix('y') {
        (stripped, 365 * 24 * 3600)
    } else if let Some(stripped) = s.strip_suffix('m') {
        (stripped, 30 * 24 * 3600)
    } else if let Some(stripped) = s.strip_suffix('w') {
        (stripped, 7 * 24 * 3600)
    } else if let Some(stripped) = s.strip_suffix('d') {
        (stripped, 24 * 3600)
    } else if let Some(stripped) = s.strip_suffix('h') {
        (stripped, 3600)
    } else {
        return None;
    };

    let num: u64 = num_part.parse().ok()?;
    Some(Duration::from_secs(num * multiplier_secs))
}
