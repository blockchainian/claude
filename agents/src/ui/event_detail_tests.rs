use super::super::inline_images::Zoom;
use super::*;

#[test]
fn image_zooms_reset_on_event_switch() {
    let mut scroll = EventDetailScroll::default();
    scroll.switch_zooms(11);
    let zoom = Zoom {
        level: 2,
        left: 0.2,
    };
    scroll.zooms.insert(0, zoom);
    scroll.switch_zooms(11);
    assert_eq!(scroll.zooms[&0], zoom);
    scroll.switch_zooms(22);
    assert!(scroll.zooms.is_empty());
    scroll.zooms.insert(0, zoom);
    scroll.switch_zooms(11);
    assert!(scroll.zooms.is_empty());
}
