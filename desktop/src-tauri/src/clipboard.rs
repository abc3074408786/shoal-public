use tauri_plugin_clipboard_manager::ClipboardExt;

#[tauri::command]
pub fn terminal_copy(webview: tauri::Webview, app: tauri::AppHandle, text: String) -> Result<(), String> {
    if webview.label() != "main" { return Err("网页不能写入系统剪贴板".into()); }
    if text.len() > 1024 * 1024 { return Err("复制文本不能超过 1 MiB".into()); }
    app.clipboard().write_text(text).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn terminal_clipboard(webview: tauri::Webview, app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    if webview.label() != "main" { return Err("网页不能读取系统剪贴板".into()); }
    let clipboard = app.clipboard();
    if let Ok(text) = clipboard.read_text() {
        if !text.is_empty() {
            if text.len() > 1024 * 1024 { return Err("粘贴文本不能超过 1 MiB".into()); }
            return Ok(serde_json::json!({"kind":"text","value":text}));
        }
    }
    let path = match clipboard.read_image() {
        Ok(image) => save_png(image.width(), image.height(), image.rgba())?,
        Err(error) => {
            #[cfg(windows)] { windows_fallback().map_err(|fallback| format!("图片读取失败：{error}；{fallback}"))? }
            #[cfg(not(windows))] { return Err(format!("图片读取失败：{error}")); }
        }
    };
    Ok(serde_json::json!({"kind":"image","value":path}))
}

#[cfg(windows)]
fn windows_fallback() -> Result<String, String> {
    // Some screenshot apps advertise a PNG/DIBV5 that arboard cannot decode,
    // while providing a valid CF_DIB. Try every available bitmap representation.
    let _guard = clipboard_win::Clipboard::new_attempts(5).map_err(|e| e.to_string())?;
    let mut errors = Vec::new();
    for format in [clipboard_win::formats::CF_DIBV5, clipboard_win::formats::CF_DIB] {
        if !clipboard_win::is_format_avail(format) { continue; }
        let mut bytes = Vec::new();
        if let Err(e) = clipboard_win::raw::get_vec(format, &mut bytes) { errors.push(e.to_string()); continue; }
        match decode_dib(&bytes) {
            Ok(image) => return save_png(image.width(), image.height(), image.as_raw()),
            Err(e) => errors.push(format!("DIB {format}: {e}")),
        }
    }
    Err(format!("没有兼容的位图格式 ({})", errors.join("; ")))
}

fn decode_dib(bytes: &[u8]) -> Result<image::RgbaImage, String> {
    use image::ImageDecoder;
    if bytes.len() > 64 * 1024 * 1024 { return Err("图片超过大小限制".into()); }
    let mut decoder = image::codecs::bmp::BmpDecoder::new_without_file_header(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let (width,height)=decoder.dimensions();
    if u64::from(width)*u64::from(height)*4 > 64*1024*1024 { return Err("图片像素超过大小限制".into()); }
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(64 * 1024 * 1024);
    limits.max_image_width = Some(16384);
    limits.max_image_height = Some(16384);
    decoder.set_limits(limits).map_err(|e| e.to_string())?;
    image::DynamicImage::from_decoder(decoder).map(|i| i.into_rgba8()).map_err(|e| e.to_string())
}

fn save_png(width: u32, height: u32, rgba: &[u8]) -> Result<String, String> {
    let bytes = u64::from(width).checked_mul(u64::from(height)).and_then(|n| n.checked_mul(4));
    if width == 0 || height == 0 || bytes.is_none_or(|n| n > 64 * 1024 * 1024 || n != rgba.len() as u64) {
        return Err("图片尺寸无效或超过 64 MiB 像素限制".into());
    }
    let mut file = tempfile::Builder::new().prefix("velella-paste-").suffix(".png").tempfile().map_err(|e| e.to_string())?;
    {
        let mut encoder = png::Encoder::new(file.as_file_mut(), width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(rgba).map_err(|e| e.to_string())?;
        writer.finish().map_err(|e| e.to_string())?;
    }
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    // Preserve the PNG while the CLI composer may still refer to it.
    let (_, path) = file.keep().map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn png_is_readable_and_preserves_pixels() {
        let path = save_png(1, 1, &[255, 0, 0, 255]).unwrap();
        let mut reader = png::Decoder::new(std::fs::File::open(&path).unwrap()).read_info().unwrap();
        let mut pixels = vec![0; reader.output_buffer_size()];
        reader.next_frame(&mut pixels).unwrap();
        assert_eq!(pixels, [255, 0, 0, 255]);
        drop(reader);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn rejects_invalid_dimensions() {
        assert!(save_png(0, 0, &[]).is_err());
        assert!(save_png(1, 1, &[0]).is_err());
        assert!(save_png(u32::MAX, u32::MAX, &[]).is_err());
    }
    #[test]
    fn legacy_dib_screenshot_decodes_without_a_bitmap_file_header() {
        let mut dib=vec![0u8;44];
        dib[0..4].copy_from_slice(&40u32.to_le_bytes());
        dib[4..8].copy_from_slice(&1i32.to_le_bytes());
        dib[8..12].copy_from_slice(&1i32.to_le_bytes());
        dib[12..14].copy_from_slice(&1u16.to_le_bytes());
        dib[14..16].copy_from_slice(&24u16.to_le_bytes());
        dib[42]=255;
        assert_eq!(decode_dib(&dib).unwrap().as_raw(),&[255,0,0,255]);
        assert!(decode_dib(&dib[..20]).is_err());
    }
    #[test]
    #[ignore = "read-only diagnostic of the user's current OS clipboard"]
    fn current_clipboard_image_read() {
        let mut clipboard = arboard::Clipboard::new().unwrap();
        let path = match clipboard.get_image() {
            Ok(image) => save_png(image.width as u32, image.height as u32, &image.bytes).unwrap(),
            Err(error) => { println!("Primary format: {error}; checking DIB fallback"); windows_fallback().unwrap() },
        };
        println!("PNG saved successfully ({} bytes)", std::fs::metadata(&path).unwrap().len());
        std::fs::remove_file(path).unwrap();
    }
}
