use super::super::inline_images::Zoom;
use super::*;

#[test]
fn image_zooms_switch_with_saved_scroll_key() {
    let mut scroll = EventDetailScroll::default();
    scroll.switch_zooms(11);
    let first = Zoom {
        level: 2,
        left: 0.2,
    };
    let second = Zoom {
        level: 1,
        left: 0.1,
    };
    scroll.zooms.insert(0, first);
    scroll.switch_zooms(22);
    assert!(scroll.zooms.is_empty());
    scroll.zooms.insert(0, second);
    scroll.switch_zooms(11);
    assert_eq!(scroll.zooms[&0], first);
    scroll.switch_zooms(11);
    assert_eq!(scroll.zooms[&0], first);
    scroll.switch_zooms(22);
    assert_eq!(scroll.zooms[&0], second);
    scroll.zooms.insert(0, Zoom::default());
    scroll.switch_zooms(11);
    scroll.switch_zooms(22);
    assert_eq!(scroll.zooms[&0], Zoom::default());
}
