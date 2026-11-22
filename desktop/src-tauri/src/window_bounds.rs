//! Keep first launch reachable on small and scaled displays.
use tauri::{PhysicalPosition, PhysicalSize, Window};

fn fit(size: (u32, u32), area: (i32, i32, u32, u32)) -> ((i32, i32), (u32, u32)) {
    let margin = 16u32.min(area.2 / 4).min(area.3 / 4);
    let width = size.0.min(area.2.saturating_sub(2 * margin)).max(1);
    let height = size.1.min(area.3.saturating_sub(2 * margin)).max(1);
    let x = area.0.saturating_add(((area.2.saturating_sub(width)) / 2) as i32);
    let y = area.1.saturating_add(((area.3.saturating_sub(height)) / 2) as i32);
    ((x, y), (width, height))
}

pub fn fit_on_start(window: &Window) -> tauri::Result<()> {
    let Some(monitor) = window.current_monitor()?.or(window.primary_monitor()?) else { return Ok(()); };
    let work = monitor.work_area();
    let area = if work.size.width > 0 && work.size.height > 0 {
        (work.position.x, work.position.y, work.size.width, work.size.height)
    } else {
        (monitor.position().x, monitor.position().y, monitor.size().width, monitor.size().height)
    };
    let current = window.inner_size()?;
    let (position, size) = fit((current.width, current.height), area);
    if size != (current.width, current.height) {
        let scale = monitor.scale_factor();
        window.set_min_size(Some(PhysicalSize::new(((960.0 * scale) as u32).min(size.0), ((640.0 * scale) as u32).min(size.1))))?;
        window.set_size(PhysicalSize::new(size.0, size.1))?;
        window.set_position(PhysicalPosition::new(position.0, position.1))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scaled_portrait_window_fits_visible_work_area() {
        let (position, size) = fit((2880, 1800), (0, 64, 1080, 2072));
        assert_eq!(size, (1048, 1800));
        assert!(position.0 >= 0 && position.1 >= 64);
        assert!(position.0 as u32 + size.0 <= 1080);
        assert!(position.1 as u32 + size.1 <= 2136);
    }
    #[test]
    fn small_and_negative_origin_displays_keep_window_inside() {
        for area in [(-1280, 0, 1280, 720), (0, 0, 320, 240)] {
            let (position, size) = fit((1440, 900), area);
            assert!(position.0 >= area.0 && position.1 >= area.1);
            assert!(i64::from(position.0) + i64::from(size.0) <= i64::from(area.0) + i64::from(area.2));
            assert!(i64::from(position.1) + i64::from(size.1) <= i64::from(area.1) + i64::from(area.3));
        }
        assert_eq!(fit((1440, 900), (0, 0, 2560, 1440)).1, (1440, 900));
    }
}
