use ratatui::symbols::border;

pub fn get_border_set(advanced_unicode_borders: bool) -> border::Set {
    if advanced_unicode_borders {
        border::Set {
            top_left: "🭽",
            top_right: "🭾",
            bottom_left: "🭼",
            bottom_right: "🭿",
            horizontal_top: "▔",    // U+2594 UPPER ONE EIGHTH BLOCK
            horizontal_bottom: "▁", // U+2581 LOWER ONE EIGHTH BLOCK
            vertical_left: "▏",     // U+258F LEFT ONE EIGHTH BLOCK
            vertical_right: "▕",    // U+2595 RIGHT ONE EIGHTH BLOCK
        }
    } else {
        border::PLAIN
    }
}
