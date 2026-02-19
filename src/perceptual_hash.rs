use std::path::Path;

use anyhow::Result;
use image::imageops::FilterType;

use crate::thumbnails::{apply_exif_orientation, get_preview_path_for_file, is_image_file};

/// Compute dHash (difference hash) for an image. Returns 64-bit perceptual hash.
///
/// Algorithm:
/// 1. Load image (prefer cached thumbnail for speed, fall back to original)
/// 2. Apply EXIF orientation correction
/// 3. Resize to 9×8 grayscale
/// 4. For each of 8 rows, compare 8 adjacent pixel pairs → 64 bits
///
/// Two visually similar images will have hashes with low Hamming distance.
pub fn compute_perceptual_hash(path: &Path) -> Result<u64> {
    if !is_image_file(path) {
        anyhow::bail!("Not an image file: {}", path.display());
    }

    let img = load_image(path)?;

    Ok(dhash_from_image(&img))
}

/// Load an image, preferring cached thumbnail for speed.
fn load_image(path: &Path) -> Result<image::DynamicImage> {
    // Try cached thumbnail first (much faster than loading full-res original)
    if let Some((preview_path, is_thumbnail)) = get_preview_path_for_file(path) {
        if is_thumbnail {
            if let Ok(img) = image::open(&preview_path) {
                // Thumbnails already have EXIF applied during generation
                return Ok(img);
            }
        }
    }

    // Fall back to original with EXIF correction
    let img = image::open(path)
        .map_err(|e| anyhow::anyhow!("Failed to open image {}: {}", path.display(), e))?;
    Ok(apply_exif_orientation(path, img))
}

/// Compute dHash from an already-loaded image.
/// Exposed for testing with synthetic images.
pub fn dhash_from_image(img: &image::DynamicImage) -> u64 {
    // Resize to 9×8 — we need 9 columns so we can compare 8 pairs per row
    let resized = img.resize_exact(9, 8, FilterType::Lanczos3);
    let gray = resized.to_luma8();

    let mut hash: u64 = 0;
    for y in 0..8 {
        for x in 0..8 {
            let left = gray.get_pixel(x, y)[0];
            let right = gray.get_pixel(x + 1, y)[0];
            if left > right {
                hash |= 1 << (y * 8 + x);
            }
        }
    }

    hash
}

/// Hamming distance between two perceptual hashes (number of differing bits).
/// Lower distance = more similar. Typically:
/// - 0: identical images
/// - 1-5: very similar (different compression/quality)
/// - 6-10: similar (minor edits, slight crop)
/// - 10+: different images
pub fn hamming_distance(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

/// Group files by perceptual hash similarity using Union-Find.
/// Returns groups of 2+ file_ids where all pairs have Hamming distance ≤ threshold.
pub fn group_by_similarity(hashes: &[(i64, u64)], threshold: u32) -> Vec<Vec<i64>> {
    let n = hashes.len();
    if n < 2 {
        return Vec::new();
    }

    let mut parent: Vec<usize> = (0..n).collect();
    let mut rank: Vec<usize> = vec![0; n];

    // Find with path compression
    fn find(parent: &mut [usize], i: usize) -> usize {
        if parent[i] != i {
            parent[i] = find(parent, parent[i]);
        }
        parent[i]
    }

    // Union by rank
    fn union(parent: &mut [usize], rank: &mut [usize], a: usize, b: usize) {
        let ra = find(parent, a);
        let rb = find(parent, b);
        if ra == rb {
            return;
        }
        match rank[ra].cmp(&rank[rb]) {
            std::cmp::Ordering::Less => parent[ra] = rb,
            std::cmp::Ordering::Greater => parent[rb] = ra,
            std::cmp::Ordering::Equal => {
                parent[rb] = ra;
                rank[ra] += 1;
            }
        }
    }

    // Compare all pairs — O(n²) but with tiny constant (popcount on u64)
    for i in 0..n {
        for j in (i + 1)..n {
            if hamming_distance(hashes[i].1, hashes[j].1) <= threshold {
                union(&mut parent, &mut rank, i, j);
            }
        }
    }

    // Collect connected components
    let mut groups: std::collections::HashMap<usize, Vec<i64>> = std::collections::HashMap::new();
    for (i, (file_id, _)) in hashes.iter().enumerate() {
        let root = find(&mut parent, i);
        groups.entry(root).or_default().push(*file_id);
    }

    // Return only groups with 2+ members
    groups
        .into_values()
        .filter(|g| g.len() >= 2)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, RgbImage};

    #[test]
    fn test_hamming_distance_identical() {
        assert_eq!(hamming_distance(0, 0), 0);
        assert_eq!(hamming_distance(u64::MAX, u64::MAX), 0);
        assert_eq!(hamming_distance(0xDEADBEEF, 0xDEADBEEF), 0);
    }

    #[test]
    fn test_hamming_distance_one_bit() {
        assert_eq!(hamming_distance(0, 1), 1);
        assert_eq!(hamming_distance(0b1000, 0b0000), 1);
    }

    #[test]
    fn test_hamming_distance_all_bits() {
        assert_eq!(hamming_distance(0, u64::MAX), 64);
    }

    #[test]
    fn test_hamming_distance_known_values() {
        // 0xFF = 8 bits set, 0x00 = 0 bits set → distance 8
        assert_eq!(hamming_distance(0xFF, 0x00), 8);
        // 0b1010 vs 0b0101 → all 4 bits differ
        assert_eq!(hamming_distance(0b1010, 0b0101), 4);
    }

    #[test]
    fn test_dhash_identical_images_same_hash() {
        let img = create_test_image(100, 80, |x, y| [x as u8, y as u8, 128]);
        let hash1 = dhash_from_image(&img);
        let hash2 = dhash_from_image(&img);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_dhash_different_sizes_same_content() {
        // Same gradient pattern at different resolutions should produce similar hashes
        let small = create_test_image(50, 40, |x, y| {
            let nx = (x * 255 / 50) as u8;
            let ny = (y * 255 / 40) as u8;
            [nx, ny, 128]
        });
        let large = create_test_image(200, 160, |x, y| {
            let nx = (x * 255 / 200) as u8;
            let ny = (y * 255 / 160) as u8;
            [nx, ny, 128]
        });

        let hash_small = dhash_from_image(&small);
        let hash_large = dhash_from_image(&large);
        let distance = hamming_distance(hash_small, hash_large);

        // Same content at different resolutions should be very close
        assert!(
            distance <= 5,
            "Same gradient at different sizes should be similar, got distance {}",
            distance
        );
    }

    #[test]
    fn test_dhash_completely_different_images() {
        // Horizontal gradient
        let img1 = create_test_image(100, 80, |x, _| {
            let v = (x * 255 / 100) as u8;
            [v, v, v]
        });
        // Checkerboard pattern
        let img2 = create_test_image(100, 80, |x, y| {
            let v = if (x / 10 + y / 10) % 2 == 0 { 255 } else { 0 };
            [v, v, v]
        });

        let hash1 = dhash_from_image(&img1);
        let hash2 = dhash_from_image(&img2);
        let distance = hamming_distance(hash1, hash2);

        assert!(
            distance > 10,
            "Completely different images should have high distance, got {}",
            distance
        );
    }

    #[test]
    fn test_dhash_slight_brightness_change() {
        // Original
        let img1 = create_test_image(100, 80, |x, y| {
            let v = ((x + y) * 255 / 180) as u8;
            [v, v, v]
        });
        // Slightly brighter (add 20 to each pixel, clamped)
        let img2 = create_test_image(100, 80, |x, y| {
            let v = (((x + y) * 255 / 180) as u16).min(235) as u8 + 20;
            [v, v, v]
        });

        let hash1 = dhash_from_image(&img1);
        let hash2 = dhash_from_image(&img2);
        let distance = hamming_distance(hash1, hash2);

        // dHash compares relative differences, so brightness shift should have minimal impact
        assert!(
            distance <= 5,
            "Slightly brighter image should be similar, got distance {}",
            distance
        );
    }

    #[test]
    fn test_group_by_similarity_empty() {
        let groups = group_by_similarity(&[], 8);
        assert!(groups.is_empty());
    }

    #[test]
    fn test_group_by_similarity_single() {
        let groups = group_by_similarity(&[(1, 0xFF)], 8);
        assert!(groups.is_empty());
    }

    #[test]
    fn test_group_by_similarity_identical_hashes() {
        let hashes = vec![(1, 0xABCD), (2, 0xABCD), (3, 0x1234)];
        let groups = group_by_similarity(&hashes, 0);
        assert_eq!(groups.len(), 1);
        let group = &groups[0];
        assert!(group.contains(&1));
        assert!(group.contains(&2));
        assert!(!group.contains(&3));
    }

    #[test]
    fn test_group_by_similarity_transitive() {
        // A is close to B, B is close to C, but A may not be close to C directly.
        // Union-Find should still group them all together.
        let a = 0b0000_0000u64;
        let b = 0b0000_0011u64; // distance 2 from a
        let c = 0b0000_1111u64; // distance 2 from b, distance 4 from a

        let hashes = vec![(1, a), (2, b), (3, c)];
        let groups = group_by_similarity(&hashes, 2);

        // All three should be in one group via transitivity
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].len(), 3);
    }

    #[test]
    fn test_group_by_similarity_separate_groups() {
        let hashes = vec![
            (1, 0x0000_0000_0000_0000u64),
            (2, 0x0000_0000_0000_0001u64), // distance 1 from #1
            (3, 0xFFFF_FFFF_FFFF_FFFFu64),
            (4, 0xFFFF_FFFF_FFFF_FFFEu64), // distance 1 from #3
        ];

        let groups = group_by_similarity(&hashes, 2);
        assert_eq!(groups.len(), 2);

        // Find which group has file_id 1
        let group_with_1 = groups.iter().find(|g| g.contains(&1)).unwrap();
        assert!(group_with_1.contains(&2));
        assert!(!group_with_1.contains(&3));

        let group_with_3 = groups.iter().find(|g| g.contains(&3)).unwrap();
        assert!(group_with_3.contains(&4));
        assert!(!group_with_3.contains(&1));
    }

    /// Helper to create a test image with a given pixel generator
    fn create_test_image(
        width: u32,
        height: u32,
        pixel_fn: impl Fn(u32, u32) -> [u8; 3],
    ) -> DynamicImage {
        let mut img = RgbImage::new(width, height);
        for y in 0..height {
            for x in 0..width {
                let [r, g, b] = pixel_fn(x, y);
                img.put_pixel(x, y, image::Rgb([r, g, b]));
            }
        }
        DynamicImage::ImageRgb8(img)
    }
}
