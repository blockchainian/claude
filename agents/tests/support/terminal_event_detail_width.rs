pub(super) fn event_detail_center(width: usize, content_width: usize) -> usize {
    (1 + (width - 2) / 2)
        .saturating_sub(content_width / 2)
        .min((width - 12).saturating_sub(content_width))
        .max(1)
}
