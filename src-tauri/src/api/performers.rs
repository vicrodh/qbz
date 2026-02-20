//! Performers string parsing utilities
//!
//! Qobuz provides performer credits as a formatted string like:
//! "John Coltrane, Saxophone, MainArtist - McCoy Tyner, Piano - Jimmy Garrison, Double Bass"
//!
//! This module parses that string into structured data.

use serde::{Deserialize, Serialize};

/// A performer with their name and roles
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Performer {
    pub name: String,
    pub roles: Vec<String>,
}

impl Performer {
    pub fn new(name: String, roles: Vec<String>) -> Self {
        Self { name, roles }
    }
}

/// Parse a Qobuz performers string into structured data
///
/// Format: "Name, Role1, Role2 - Name, Role1 - Name, Role1, Role2"
///
/// # Examples
///
/// ```
/// use crate::api::performers::parse_performers;
///
/// let performers = parse_performers("John Coltrane, Saxophone, MainArtist - McCoy Tyner, Piano");
/// assert_eq!(performers.len(), 2);
/// assert_eq!(performers[0].name, "John Coltrane");
/// assert_eq!(performers[0].roles, vec!["Saxophone", "MainArtist"]);
/// ```
pub fn parse_performers(performers_str: &str) -> Vec<Performer> {
    if performers_str.is_empty() {
        return Vec::new();
    }

    performers_str
        .split(" - ")
        .filter_map(|segment| {
            let segment = segment.trim();
            if segment.is_empty() {
                return None;
            }

            let parts: Vec<&str> = segment.split(", ").collect();
            if parts.is_empty() {
                return None;
            }

            let name = parts[0].trim().to_string();
            if name.is_empty() {
                return None;
            }

            let roles: Vec<String> = parts[1..]
                .iter()
                .map(|r| r.trim().to_string())
                .filter(|r| !r.is_empty())
                .collect();

            Some(Performer::new(name, roles))
        })
        .collect()
}

/// Group performers by their roles
///
/// Returns a map where keys are role names and values are lists of performer names
pub fn group_by_role(performers: &[Performer]) -> std::collections::HashMap<String, Vec<String>> {
    let mut grouped: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();

    for performer in performers {
        for role in &performer.roles {
            grouped
                .entry(role.clone())
                .or_default()
                .push(performer.name.clone());
        }
    }

    grouped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single_performer() {
        let result = parse_performers("John Coltrane, Saxophone");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "John Coltrane");
        assert_eq!(result[0].roles, vec!["Saxophone"]);
    }

    #[test]
    fn test_parse_multiple_performers() {
        let result = parse_performers(
            "John Coltrane, Saxophone, MainArtist - McCoy Tyner, Piano - Elvin Jones, Drums",
        );
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].name, "John Coltrane");
        assert_eq!(result[0].roles, vec!["Saxophone", "MainArtist"]);
        assert_eq!(result[1].name, "McCoy Tyner");
        assert_eq!(result[1].roles, vec!["Piano"]);
        assert_eq!(result[2].name, "Elvin Jones");
        assert_eq!(result[2].roles, vec!["Drums"]);
    }

    #[test]
    fn test_parse_empty_string() {
        let result = parse_performers("");
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_performer_no_roles() {
        let result = parse_performers("John Coltrane");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "John Coltrane");
        assert!(result[0].roles.is_empty());
    }

    #[test]
    fn test_group_by_role() {
        let performers = vec![
            Performer::new("John".to_string(), vec!["Saxophone".to_string()]),
            Performer::new(
                "Jane".to_string(),
                vec!["Saxophone".to_string(), "Vocals".to_string()],
            ),
        ];
        let grouped = group_by_role(&performers);
        assert_eq!(grouped.get("Saxophone").unwrap().len(), 2);
        assert_eq!(grouped.get("Vocals").unwrap().len(), 1);
    }
}
