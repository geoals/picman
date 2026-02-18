use std::path::Path;

/// Parsed EXIF metadata for display in the expanded details panel
#[derive(Debug, Clone, Default)]
pub struct ExifInfo {
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub lens: Option<String>,
    pub aperture: Option<String>,
    pub shutter_speed: Option<String>,
    pub iso: Option<String>,
    pub focal_length: Option<String>,
    pub gps_lat: Option<f64>,
    pub gps_lon: Option<f64>,
}

impl ExifInfo {
    pub fn has_any(&self) -> bool {
        self.camera_make.is_some()
            || self.camera_model.is_some()
            || self.lens.is_some()
            || self.aperture.is_some()
            || self.shutter_speed.is_some()
            || self.iso.is_some()
            || self.focal_length.is_some()
            || self.gps_lat.is_some()
    }
}

/// Read EXIF metadata from an image file (header-only, fast)
pub fn read_exif(path: &Path) -> ExifInfo {
    let Ok(file) = std::fs::File::open(path) else {
        return ExifInfo::default();
    };
    let mut bufreader = std::io::BufReader::new(file);
    let Ok(exif) = exif::Reader::new().read_from_container(&mut bufreader) else {
        return ExifInfo::default();
    };

    let get_str = |tag: exif::Tag| -> Option<String> {
        let field = exif.get_field(tag, exif::In::PRIMARY)?;
        let s = field.display_value().with_unit(&exif).to_string();
        if s.is_empty() || s == "\"\"" {
            None
        } else {
            // Strip surrounding quotes that kamadak-exif adds to ASCII strings
            Some(s.trim_matches('"').to_string())
        }
    };

    let get_rational = |tag: exif::Tag| -> Option<f64> {
        let field = exif.get_field(tag, exif::In::PRIMARY)?;
        match &field.value {
            exif::Value::Rational(vals) => vals.first().map(|r| r.to_f64()),
            _ => None,
        }
    };

    let gps_lat = parse_gps_coord(&exif, exif::Tag::GPSLatitude, exif::Tag::GPSLatitudeRef);
    let gps_lon = parse_gps_coord(&exif, exif::Tag::GPSLongitude, exif::Tag::GPSLongitudeRef);

    ExifInfo {
        camera_make: get_str(exif::Tag::Make),
        camera_model: get_str(exif::Tag::Model),
        lens: get_str(exif::Tag::LensModel).or_else(|| get_str(exif::Tag::LensSpecification)),
        aperture: get_rational(exif::Tag::FNumber).map(|f| format!("f/{:.1}", f)),
        shutter_speed: get_str(exif::Tag::ExposureTime).map(|s| format!("{} s", s)),
        iso: get_str(exif::Tag::PhotographicSensitivity)
            .or_else(|| get_str(exif::Tag::ISOSpeed))
            .map(|s| format!("ISO {}", s)),
        focal_length: get_rational(exif::Tag::FocalLength).map(|f| format!("{:.0} mm", f)),
        gps_lat,
        gps_lon,
    }
}

/// Parse GPS coordinate from EXIF (degrees, minutes, seconds + reference direction)
fn parse_gps_coord(
    exif: &exif::Exif,
    coord_tag: exif::Tag,
    ref_tag: exif::Tag,
) -> Option<f64> {
    let field = exif.get_field(coord_tag, exif::In::PRIMARY)?;
    let rationals = match &field.value {
        exif::Value::Rational(vals) if vals.len() >= 3 => vals,
        _ => return None,
    };

    let degrees = rationals[0].to_f64();
    let minutes = rationals[1].to_f64();
    let seconds = rationals[2].to_f64();
    let mut coord = degrees + minutes / 60.0 + seconds / 3600.0;

    // Apply direction (S and W are negative)
    if let Some(ref_field) = exif.get_field(ref_tag, exif::In::PRIMARY) {
        let dir = ref_field.display_value().to_string();
        if dir == "S" || dir == "W" {
            coord = -coord;
        }
    }

    Some(coord)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_exif_nonexistent_file() {
        let info = read_exif(Path::new("/nonexistent/file.jpg"));
        assert!(!info.has_any());
    }

    #[test]
    fn test_read_exif_non_image_file() {
        let temp = tempfile::TempDir::new().unwrap();
        let path = temp.path().join("test.txt");
        std::fs::write(&path, "not an image").unwrap();

        let info = read_exif(&path);
        assert!(!info.has_any());
    }

    #[test]
    fn test_exif_info_has_any() {
        let empty = ExifInfo::default();
        assert!(!empty.has_any());

        let with_make = ExifInfo {
            camera_make: Some("Canon".to_string()),
            ..Default::default()
        };
        assert!(with_make.has_any());
    }
}
