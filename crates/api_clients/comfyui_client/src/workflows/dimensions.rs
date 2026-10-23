/// Roughly one-megapixel sizes, all multiples of 64, that both SDXL and FLUX
/// were trained around. Ordered from widest to tallest.
const ONE_MEGAPIXEL_BUCKETS: &[(u32, u32)] = &[
  (1536, 640),  // 12:5 (~21:9)
  (1344, 768),  // 7:4  (~16:9)
  (1216, 832),  // 19:13 (~3:2)
  (1152, 896),  // 9:7  (~4:3, 5:4)
  (1024, 1024), // 1:1
  (896, 1152),  // 7:9  (~3:4, 4:5)
  (832, 1216),  // (~2:3)
  (768, 1344),  // (~9:16)
  (640, 1536),  // (~9:21)
];

/// Pick the trained size closest to the requested aspect ratio
/// (e.g. `16, 9` → `1344×768`).
pub fn dimensions_for_aspect_ratio(width_ratio: u32, height_ratio: u32) -> (u32, u32) {
  if width_ratio == 0 || height_ratio == 0 {
    return (1024, 1024);
  }
  let target = (width_ratio as f64 / height_ratio as f64).ln();
  *ONE_MEGAPIXEL_BUCKETS
    .iter()
    .min_by(|a, b| {
      let da = ((a.0 as f64 / a.1 as f64).ln() - target).abs();
      let db = ((b.0 as f64 / b.1 as f64).ln() - target).abs();
      da.total_cmp(&db)
    })
    .expect("buckets are not empty")
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn common_ratios() {
    assert_eq!(dimensions_for_aspect_ratio(1, 1), (1024, 1024));
    assert_eq!(dimensions_for_aspect_ratio(16, 9), (1344, 768));
    assert_eq!(dimensions_for_aspect_ratio(9, 16), (768, 1344));
    assert_eq!(dimensions_for_aspect_ratio(4, 3), (1152, 896));
    assert_eq!(dimensions_for_aspect_ratio(3, 4), (896, 1152));
    assert_eq!(dimensions_for_aspect_ratio(3, 2), (1216, 832));
    assert_eq!(dimensions_for_aspect_ratio(2, 3), (832, 1216));
    assert_eq!(dimensions_for_aspect_ratio(21, 9), (1536, 640));
  }

  #[test]
  fn degenerate_ratio_is_square() {
    assert_eq!(dimensions_for_aspect_ratio(0, 9), (1024, 1024));
  }

  #[test]
  fn every_bucket_is_a_multiple_of_64() {
    assert!(ONE_MEGAPIXEL_BUCKETS.iter().all(|(w, h)| w % 64 == 0 && h % 64 == 0));
  }
}
