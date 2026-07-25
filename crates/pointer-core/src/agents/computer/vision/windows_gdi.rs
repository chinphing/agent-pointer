//! Windows GDI desktop capture fallback when xcap WGC / D3D11 is unavailable (RDP, VM, E_OUTOFMEMORY).

use anyhow::{anyhow, Result};
use image::RgbaImage;
use std::mem;
use windows::Win32::{
    Foundation::GetLastError,
    Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits,
        GetWindowDC, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS,
        SRCCOPY,
    },
    UI::WindowsAndMessaging::GetDesktopWindow,
};

/// Capture a desktop rectangle in global screen coordinates via GDI BitBlt.
pub fn capture_monitor_region(x: i32, y: i32, width: i32, height: i32) -> Result<RgbaImage> {
    if width <= 0 || height <= 0 {
        return Err(anyhow!("invalid GDI capture size {width}x{height}"));
    }

    unsafe {
        let hwnd = GetDesktopWindow();
        let hdc_desktop = GetWindowDC(Some(hwnd));
        if hdc_desktop.is_invalid() {
            return Err(anyhow!("GetWindowDC failed: {:?}", GetLastError()));
        }

        let hdc_mem = CreateCompatibleDC(Some(hdc_desktop));
        if hdc_mem.is_invalid() {
            ReleaseDC(Some(hwnd), hdc_desktop);
            return Err(anyhow!("CreateCompatibleDC failed: {:?}", GetLastError()));
        }

        let hbitmap = CreateCompatibleBitmap(hdc_desktop, width, height);
        if hbitmap.is_invalid() {
            let _ = DeleteDC(hdc_mem);
            ReleaseDC(Some(hwnd), hdc_desktop);
            return Err(anyhow!(
                "CreateCompatibleBitmap failed: {:?}",
                GetLastError()
            ));
        }

        let _prev = SelectObject(hdc_mem, hbitmap.into());

        if BitBlt(
            hdc_mem,
            0,
            0,
            width,
            height,
            Some(hdc_desktop),
            x,
            y,
            SRCCOPY,
        )
        .is_err()
        {
            let _ = DeleteObject(hbitmap.into());
            let _ = DeleteDC(hdc_mem);
            ReleaseDC(Some(hwnd), hdc_desktop);
            return Err(anyhow!("BitBlt failed: {:?}", GetLastError()));
        }

        ReleaseDC(Some(hwnd), hdc_desktop);

        let rgba = read_bitmap_rgba(hdc_mem, hbitmap, width, height)?;

        let _ = DeleteObject(hbitmap.into());
        let _ = DeleteDC(hdc_mem);

        Ok(rgba)
    }
}

unsafe fn read_bitmap_rgba(
    hdc_mem: windows::Win32::Graphics::Gdi::HDC,
    hbitmap: windows::Win32::Graphics::Gdi::HBITMAP,
    width: i32,
    height: i32,
) -> Result<RgbaImage> {
    let buffer_size = (width * height * 4) as usize;
    let mut bitmap_info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biSizeImage: buffer_size as u32,
            biCompression: 0,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut buffer = vec![0u8; buffer_size];
    if GetDIBits(
        hdc_mem,
        hbitmap,
        0,
        height as u32,
        Some(buffer.as_mut_ptr().cast()),
        &mut bitmap_info,
        DIB_RGB_COLORS,
    ) == 0
    {
        return Err(anyhow!("GetDIBits failed: {:?}", GetLastError()));
    }

    bgra_to_rgba(buffer).and_then(|pixels| {
        RgbaImage::from_raw(width as u32, height as u32, pixels)
            .ok_or_else(|| anyhow!("RgbaImage::from_raw failed"))
    })
}

fn bgra_to_rgba(mut buffer: Vec<u8>) -> Result<Vec<u8>> {
    if buffer.len() % 4 != 0 {
        return Err(anyhow!("invalid BGRA buffer length {}", buffer.len()));
    }
    for chunk in buffer.chunks_exact_mut(4) {
        chunk.swap(0, 2);
    }
    Ok(buffer)
}
