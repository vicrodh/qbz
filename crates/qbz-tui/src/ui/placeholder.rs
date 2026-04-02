use ratatui::layout::{Alignment, Rect};
use ratatui::style::Style;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::theme::TEXT_MUTED;

/// Render a centered placeholder label for a view that is not yet implemented.
pub fn render_placeholder(frame: &mut Frame, area: Rect, title: &str) {
    // Vertically center: place the text at the middle row
    let mid_y = area.y + area.height / 2;
    if mid_y < area.y + area.height {
        let row = Rect::new(area.x, mid_y, area.width, 1);
        let paragraph = Paragraph::new(title)
            .style(Style::default().fg(TEXT_MUTED))
            .alignment(Alignment::Center);
        frame.render_widget(paragraph, row);
    }
}
