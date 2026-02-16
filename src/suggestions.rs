//! Word suggestion extraction for rename dialog.
//!
//! Extracts meaningful words from directory paths and file tags
//! to suggest rename options.

use std::collections::{HashMap, HashSet};

/// Extract meaningful words from a directory path for rename suggestions.
/// Words from file tags are weighted higher (counted 3x).
pub fn extract_suggested_words(path: &str, file_tags: &[String]) -> Vec<String> {
    let mut word_counts: HashMap<String, usize> = HashMap::new();

    // Delimiters for splitting
    let delimiters = [
        ' ', '-', '_', '@', '(', ')', '[', ']', '{', '}', '【', '】', '「', '」', '『', '』', '/',
        '\\', '&', '+',
    ];

    // Noise words to filter out
    let noise: HashSet<&str> = [
        "no", "vol", "p", "v", "gb", "mb", "kb", "pic", "video", "gif", "cosplay", "coser", "ver",
        "version", "normal", "bonus", "set", "part", "作品", "月", "年", "订阅", "特典", "合集",
    ]
    .iter()
    .copied()
    .collect();

    // Process path segments
    for segment in path.split('/') {
        let mut current_word = String::new();

        for c in segment.chars() {
            if delimiters.contains(&c) {
                if !current_word.is_empty() {
                    process_word(&current_word, &noise, &mut word_counts);
                    current_word.clear();
                }
            } else {
                current_word.push(c);
            }
        }

        if !current_word.is_empty() {
            process_word(&current_word, &noise, &mut word_counts);
        }
    }

    // Add file tags with high weight
    for tag in file_tags {
        *word_counts.entry(tag.clone()).or_insert(0) += 3;
    }

    // Sort by frequency, then alphabetically
    let mut words: Vec<(String, usize)> = word_counts.into_iter().collect();
    words.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    words.into_iter().map(|(w, _)| w).take(30).collect()
}

fn process_word(word: &str, noise: &HashSet<&str>, counts: &mut HashMap<String, usize>) {
    let trimmed = word.trim();
    let lower = trimmed.to_lowercase();

    // Skip if too short
    if trimmed.len() < 2 {
        return;
    }

    // Skip if noise word
    if noise.contains(lower.as_str()) {
        return;
    }

    // Skip if purely numeric
    if trimmed.chars().all(|c| c.is_ascii_digit()) {
        return;
    }

    // Skip if looks like file size (e.g., "1.37GB", "350MB")
    if trimmed.ends_with("GB") || trimmed.ends_with("MB") || trimmed.ends_with("KB") {
        return;
    }

    // Skip patterns like "73P1V" or "45P"
    if trimmed.chars().any(|c| c.is_ascii_digit())
        && (trimmed.contains('P') || trimmed.contains('V'))
        && trimmed.len() < 10
    {
        return;
    }

    *counts.entry(trimmed.to_string()).or_insert(0) += 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extracts_words_from_path_segments() {
        let result = extract_suggested_words("photos/Alice-Wonderland/shoot1", &[]);
        assert!(result.contains(&"photos".to_string()));
        assert!(result.contains(&"Alice".to_string()));
        assert!(result.contains(&"Wonderland".to_string()));
        assert!(result.contains(&"shoot1".to_string()));
    }

    #[test]
    fn test_filters_noise_words() {
        let result = extract_suggested_words("model_vol_2_bonus_set", &[]);
        assert!(!result.contains(&"vol".to_string()));
        assert!(!result.contains(&"bonus".to_string()));
        assert!(!result.contains(&"set".to_string()));
        assert!(result.contains(&"model".to_string()));
    }

    #[test]
    fn test_filters_numeric_only() {
        let result = extract_suggested_words("shoot_2024_01_15_photos", &[]);
        assert!(!result.contains(&"2024".to_string()));
        assert!(!result.contains(&"01".to_string()));
        assert!(!result.contains(&"15".to_string()));
        assert!(result.contains(&"shoot".to_string()));
        assert!(result.contains(&"photos".to_string()));
    }

    #[test]
    fn test_filters_file_sizes() {
        let result = extract_suggested_words("photoset_1.37GB_highres", &[]);
        assert!(!result.contains(&"1.37GB".to_string()));
        assert!(result.contains(&"photoset".to_string()));
        assert!(result.contains(&"highres".to_string()));
    }

    #[test]
    fn test_weights_file_tags_higher() {
        // When a word appears once in path and a tag is provided,
        // the tag should rank higher due to 3x weight
        let result = extract_suggested_words("alpha/beta/beta", &["mytag".to_string()]);

        // mytag has weight 3, beta has weight 2, alpha has weight 1
        // Order should be: mytag, beta, alpha
        assert_eq!(result[0], "mytag");
        assert_eq!(result[1], "beta");
        assert_eq!(result[2], "alpha");
    }

    #[test]
    fn test_limits_to_30_results() {
        // Create a path with many unique words
        let words: Vec<String> = (0..50).map(|i| format!("word{:02}", i)).collect();
        let path = words.join("/");
        let result = extract_suggested_words(&path, &[]);
        assert_eq!(result.len(), 30);
    }

    #[test]
    fn test_handles_unicode_delimiters() {
        let result = extract_suggested_words("【Model】「Photoshoot」『Album』", &[]);
        assert!(result.contains(&"Model".to_string()));
        assert!(result.contains(&"Photoshoot".to_string()));
        assert!(result.contains(&"Album".to_string()));
    }
}
