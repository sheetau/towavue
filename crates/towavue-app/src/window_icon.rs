use winit::{
    dpi::PhysicalSize,
    platform::windows::{IconExtWindows, WindowExtWindows},
    window::{BadIcon, Icon, Window},
};

fn load(scale: f64) -> Result<(Icon, Icon), BadIcon> {
    let icon = |logical: f64| {
        let size = (logical * scale).round() as u32;
        Icon::from_resource(1, Some(PhysicalSize::new(size, size)))
    };
    Ok((icon(16.0)?, icon(32.0)?))
}

pub fn apply(window: &Window, scale: f64) {
    match load(scale) {
        Ok((small, large)) => {
            window.set_window_icon(Some(small));
            window.set_taskbar_icon(Some(large));
        }
        Err(error) => eprintln!("Could not load application icon: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_icon_loads_for_each_window_density() {
        for scale in [1.0, 1.25, 1.5, 2.0, 3.0, 4.0] {
            load(scale).expect("embedded small and taskbar icons");
        }
        for size in [16, 32, 48, 64, 128, 256] {
            Icon::from_resource(1, Some(PhysicalSize::new(size, size)))
                .expect("each supplied icon size");
        }
    }
}
