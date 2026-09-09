use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::Arc;

use winit::dpi::PhysicalPosition;
use winit::window::{CursorGrabMode, Window};

/// UI-thread cursor lock. Retains its window until the lock is released on drop.
pub struct PinnedCursor {
    window: Arc<Window>,
    _ui_thread: PhantomData<Rc<()>>,
}

impl PinnedCursor {
    pub fn new(window: Arc<Window>, position: (f64, f64)) -> Result<Self, String> {
        let size = window.inner_size();
        if !window.has_focus()
            || !(0.0..f64::from(size.width)).contains(&position.0)
            || !(0.0..f64::from(size.height)).contains(&position.1)
        {
            return Err("Cursor locking requires a focused window and an interior position".into());
        }
        let cursor = Self {
            window,
            _ui_thread: PhantomData,
        };
        cursor
            .window
            .set_cursor_position(PhysicalPosition::new(position.0, position.1))
            .map_err(|error| error.to_string())?;
        cursor
            .window
            .set_cursor_grab(CursorGrabMode::Locked)
            .map_err(|error| error.to_string())?;
        Ok(cursor)
    }
}

impl Drop for PinnedCursor {
    fn drop(&mut self) {
        let _ = self.window.set_cursor_grab(CursorGrabMode::None);
    }
}
