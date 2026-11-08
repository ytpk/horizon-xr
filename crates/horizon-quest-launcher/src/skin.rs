//! The face of a skin for the TV: the 8 by 8 head of a 64-pixel skin with its hat layer over it,
//! from the skin's address in the account's profile (textures.minecraft.net, public).
use std::time::Duration;

/// The head's RGBA, 8 by 8, of a skin PNG: the face at (8, 8) and the hat layer at (40, 8) over
/// it where the hat is not transparent. None for anything that is not a 64-pixel-wide skin.
pub fn face_from_png(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    if info.bit_depth != png::BitDepth::Eight || info.width != 64 || info.height < 16 {
        return None;
    }
    let channels = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        _ => return None,
    };
    let pixel = |x: usize, y: usize| -> [u8; 4] {
        let at = (y * 64 + x) * channels;
        let p = &buf[at..at + channels];
        [p[0], p[1], p[2], if channels == 4 { p[3] } else { 255 }]
    };
    let mut out = Vec::with_capacity(8 * 8 * 4);
    for y in 0..8 {
        for x in 0..8 {
            let face = pixel(8 + x, 8 + y);
            let hat = pixel(40 + x, 8 + y);
            out.extend(if hat[3] >= 128 { hat } else { face });
        }
    }
    Some(out)
}

/// Downloads the skin at `url` (only Mojang's texture server is asked) and returns its face.
pub fn fetch_face(url: &str, user_agent: &str) -> Result<Vec<u8>, String> {
    if !url.starts_with("https://textures.minecraft.net/") {
        return Err("not a Mojang texture address".into());
    }
    let response = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(20))
        .user_agent(user_agent)
        .build()
        .get(url)
        .call()
        .map_err(|_| "no connection".to_owned())?;
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(
        &mut std::io::Read::take(response.into_reader(), 1 << 20),
        &mut bytes,
    )
    .map_err(|e| e.to_string())?;
    face_from_png(&bytes).ok_or_else(|| "not a skin".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skin(pixel: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, 64, 64);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut data = Vec::new();
        for y in 0..64 {
            for x in 0..64 {
                data.extend(pixel(x, y));
            }
        }
        enc.write_header().unwrap().write_image_data(&data).unwrap();
        out
    }

    #[test]
    fn the_face_is_the_8_by_8_at_8_8_with_the_hat_over_it_where_the_hat_is_solid() {
        let bytes = skin(|x, y| match (x, y) {
            // The face: a red column on its left edge; the hat: blue on one pixel, clear elsewhere.
            (8, 8..=15) => [255, 0, 0, 255],
            (40, 8) => [0, 0, 255, 255],
            (8..=15, 8..=15) => [10, 200, 10, 255],
            _ => [0, 0, 0, 0],
        });
        let face = face_from_png(&bytes).unwrap();
        assert_eq!(face.len(), 256);
        assert_eq!(
            &face[0..4],
            &[0, 0, 255, 255],
            "the hat's pixel covers the face's"
        );
        assert_eq!(
            &face[(8 * 4)..(8 * 4 + 4)],
            &[255, 0, 0, 255],
            "red column, second row"
        );
        assert_eq!(&face[4..8], &[10, 200, 10, 255]);
    }

    #[test]
    fn something_that_is_not_a_skin_gives_no_face_and_only_mojang_is_asked() {
        assert_eq!(face_from_png(b"not a png"), None);
        let mut small = Vec::new();
        let mut enc = png::Encoder::new(&mut small, 8, 8);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header()
            .unwrap()
            .write_image_data(&[0u8; 256])
            .unwrap();
        assert_eq!(face_from_png(&small), None);
        assert!(fetch_face("https://example.com/skin.png", "t").is_err());
    }
}
