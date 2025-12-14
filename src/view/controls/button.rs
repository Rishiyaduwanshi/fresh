//! Button control for triggering actions
//!
//! Renders as: `[ Button Text ]`

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use super::FocusState;

/// State for a button control
#[derive(Debug, Clone)]
pub struct ButtonState {
    /// Button label text
    pub label: String,
    /// Focus state
    pub focus: FocusState,
    /// Whether the button is currently pressed (for visual feedback)
    pub pressed: bool,
}

impl ButtonState {
    /// Create a new button state
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            focus: FocusState::Normal,
            pressed: false,
        }
    }

    /// Set the focus state
    pub fn with_focus(mut self, focus: FocusState) -> Self {
        self.focus = focus;
        self
    }

    /// Check if the button can be activated
    pub fn is_enabled(&self) -> bool {
        self.focus != FocusState::Disabled
    }

    /// Set pressed state (for visual feedback)
    pub fn set_pressed(&mut self, pressed: bool) {
        self.pressed = pressed;
    }
}

/// Colors for the button control
#[derive(Debug, Clone, Copy)]
pub struct ButtonColors {
    /// Button text color
    pub text: Color,
    /// Border color
    pub border: Color,
    /// Background color (when pressed)
    pub pressed_bg: Color,
    /// Focused highlight color
    pub focused: Color,
    /// Hovered highlight color
    pub hovered: Color,
    /// Disabled color
    pub disabled: Color,
}

impl Default for ButtonColors {
    fn default() -> Self {
        Self {
            text: Color::White,
            border: Color::Gray,
            pressed_bg: Color::DarkGray,
            focused: Color::Cyan,
            hovered: Color::Blue,
            disabled: Color::DarkGray,
        }
    }
}

impl ButtonColors {
    /// Create colors from theme
    pub fn from_theme(theme: &crate::view::theme::Theme) -> Self {
        Self {
            text: theme.editor_fg,
            border: theme.line_number_fg,
            pressed_bg: theme.selection_bg,
            focused: theme.selection_bg,
            hovered: theme.menu_hover_bg,
            disabled: theme.line_number_fg,
        }
    }

    /// Create a primary/accent button style
    pub fn primary() -> Self {
        Self {
            text: Color::Black,
            border: Color::Cyan,
            pressed_bg: Color::LightCyan,
            focused: Color::Cyan,
            hovered: Color::LightCyan,
            disabled: Color::DarkGray,
        }
    }

    /// Create a danger/destructive button style
    pub fn danger() -> Self {
        Self {
            text: Color::White,
            border: Color::Red,
            pressed_bg: Color::LightRed,
            focused: Color::Red,
            hovered: Color::LightRed,
            disabled: Color::DarkGray,
        }
    }
}

/// Layout information returned after rendering for hit testing
#[derive(Debug, Clone, Copy)]
pub struct ButtonLayout {
    /// The clickable button area
    pub button_area: Rect,
}

impl ButtonLayout {
    /// Check if a point is within the button
    pub fn contains(&self, x: u16, y: u16) -> bool {
        x >= self.button_area.x
            && x < self.button_area.x + self.button_area.width
            && y >= self.button_area.y
            && y < self.button_area.y + self.button_area.height
    }
}

/// Render a button control
///
/// # Arguments
/// * `frame` - The ratatui frame to render to
/// * `area` - Rectangle where the button should be rendered
/// * `state` - The button state
/// * `colors` - Colors for rendering
///
/// # Returns
/// Layout information for hit testing
pub fn render_button(
    frame: &mut Frame,
    area: Rect,
    state: &ButtonState,
    colors: &ButtonColors,
) -> ButtonLayout {
    if area.height == 0 || area.width < 4 {
        return ButtonLayout {
            button_area: Rect::default(),
        };
    }

    let (text_color, border_color, bg_color) = match state.focus {
        FocusState::Normal => {
            if state.pressed {
                (colors.text, colors.border, Some(colors.pressed_bg))
            } else {
                (colors.text, colors.border, None)
            }
        }
        FocusState::Focused => {
            if state.pressed {
                (colors.text, colors.focused, Some(colors.pressed_bg))
            } else {
                (colors.focused, colors.focused, None)
            }
        }
        FocusState::Hovered => {
            // Hover uses dedicated hover color from theme
            (colors.hovered, colors.hovered, None)
        }
        FocusState::Disabled => (colors.disabled, colors.disabled, None),
    };

    // Calculate button width: "[ " + label + " ]"
    let button_width = (state.label.len() + 4) as u16;
    let actual_width = button_width.min(area.width);

    // Truncate label if needed
    let max_label_len = actual_width.saturating_sub(4) as usize;
    let display_label: String = state.label.chars().take(max_label_len).collect();

    let mut style = Style::default().fg(text_color);
    if let Some(bg) = bg_color {
        style = style.bg(bg);
    }
    if state.focus == FocusState::Focused {
        style = style.add_modifier(Modifier::BOLD);
    }

    let line = Line::from(vec![
        Span::styled("[", Style::default().fg(border_color)),
        Span::raw(" "),
        Span::styled(&display_label, style),
        Span::raw(" "),
        Span::styled("]", Style::default().fg(border_color)),
    ]);

    let button_area = Rect::new(area.x, area.y, actual_width, 1);
    let paragraph = Paragraph::new(line);
    frame.render_widget(paragraph, button_area);

    ButtonLayout { button_area }
}

/// Render a row of buttons with equal spacing
///
/// # Arguments
/// * `frame` - The ratatui frame to render to
/// * `area` - Rectangle where the buttons should be rendered
/// * `buttons` - Slice of (state, colors) tuples for each button
/// * `gap` - Space between buttons
///
/// # Returns
/// Layout information for each button
pub fn render_button_row(
    frame: &mut Frame,
    area: Rect,
    buttons: &[(&ButtonState, &ButtonColors)],
    gap: u16,
) -> Vec<ButtonLayout> {
    if buttons.is_empty() || area.height == 0 {
        return Vec::new();
    }

    let mut layouts = Vec::with_capacity(buttons.len());
    let mut x = area.x;

    for (state, colors) in buttons {
        let button_width = (state.label.len() + 4) as u16;
        if x + button_width > area.x + area.width {
            break;
        }

        let button_area = Rect::new(x, area.y, button_width, 1);
        let layout = render_button(frame, button_area, state, colors);
        layouts.push(layout);

        x += button_width + gap;
    }

    layouts
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn test_frame<F>(width: u16, height: u16, f: F)
    where
        F: FnOnce(&mut Frame, Rect),
    {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = Rect::new(0, 0, width, height);
                f(frame, area);
            })
            .unwrap();
    }

    #[test]
    fn test_button_renders() {
        test_frame(20, 1, |frame, area| {
            let state = ButtonState::new("OK");
            let colors = ButtonColors::default();
            let layout = render_button(frame, area, &state, &colors);

            assert_eq!(layout.button_area.width, 6); // "[ OK ]"
        });
    }

    #[test]
    fn test_button_hit_detection() {
        test_frame(20, 1, |frame, area| {
            let state = ButtonState::new("Click");
            let colors = ButtonColors::default();
            let layout = render_button(frame, area, &state, &colors);

            // Inside button
            assert!(layout.contains(0, 0));
            assert!(layout.contains(5, 0));

            // Outside button
            assert!(!layout.contains(15, 0));
        });
    }

    #[test]
    fn test_button_row() {
        test_frame(40, 1, |frame, area| {
            let ok = ButtonState::new("OK");
            let cancel = ButtonState::new("Cancel");
            let colors = ButtonColors::default();

            let layouts =
                render_button_row(frame, area, &[(&ok, &colors), (&cancel, &colors)], 2);

            assert_eq!(layouts.len(), 2);
            assert!(layouts[0].button_area.x < layouts[1].button_area.x);
        });
    }

    #[test]
    fn test_button_disabled() {
        let state = ButtonState::new("Save").with_focus(FocusState::Disabled);
        assert!(!state.is_enabled());
    }

    #[test]
    fn test_button_pressed_state() {
        let mut state = ButtonState::new("Submit");
        assert!(!state.pressed);

        state.set_pressed(true);
        assert!(state.pressed);
    }

    #[test]
    fn test_button_truncation() {
        test_frame(8, 1, |frame, area| {
            let state = ButtonState::new("Very Long Button Text");
            let colors = ButtonColors::default();
            let layout = render_button(frame, area, &state, &colors);

            // Button should be truncated to fit
            assert!(layout.button_area.width <= area.width);
        });
    }
}
