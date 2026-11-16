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
    NewerThan(Duration),
    OlderThan(Duration),
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
    pub name_patterns: Vec<(Pattern, bool)>,
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

        let tokens = tokenize_query(trimmed);

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
                if val_str.ends_with('d')
                    || val_str.ends_with('h')
                    || val_str.ends_with('m')
                    || val_str.ends_with('w')
                    || val_str.ends_with('y')
                {
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
                let (pattern_str, negated) = if let Some(p) = token.strip_prefix('!') {
                    (p, true)
                } else {
                    (token.as_str(), false)
                };

                if pattern_str.is_empty() {
                    continue;
                }

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

    pub fn is_empty(&self) -> bool {
        self.raw_query.is_empty()
    }

    pub fn matches(&self, node: &FileNode) -> bool {
        if self.raw_query.is_empty() {
            return true;
        }

        if self.dirs_only && !node.is_dir() {
            return false;
        }
        if self.files_only && node.is_dir() {
            return false;
        }

        if node.is_dir() && !self.dirs_only {
            return true;
        }

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

        if !self.age_filters.is_empty() {
            let Some(mod_time) = node.modified else {
                return false;
            };
            let elapsed = SystemTime::now()
                .duration_since(mod_time)
                .unwrap_or(Duration::ZERO);
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

        true
    }
}

fn tokenize_query(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut quote_char = '"';

    for c in input.chars() {
        if in_quotes {
            if c == quote_char {
                in_quotes = false;
            } else {
                current.push(c);
            }
        } else if c == '"' || c == '\'' {
            in_quotes = true;
            quote_char = c;
        } else if c == ';' || c.is_whitespace() {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
        } else {
            current.push(c);
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
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
    if !num.is_finite() || num < 0.0 {
        return None;
    }
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
    Some(Duration::from_secs(num.saturating_mul(multiplier_secs)))
}
