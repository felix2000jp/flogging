use ratatui::style::Color;

pub(super) const BACKGROUND: Color = Color::Rgb(22, 22, 30);
pub(super) const PANEL_BACKGROUND: Color = Color::Rgb(26, 27, 38);
pub(super) const SELECTED_BACKGROUND: Color = Color::Rgb(41, 46, 66);
pub(super) const INACTIVE_BORDER: Color = Color::Rgb(65, 72, 104);
pub(super) const DIM_TEXT: Color = Color::Rgb(86, 95, 137);
pub(super) const SECONDARY_TEXT: Color = Color::Rgb(169, 177, 214);
pub(super) const PRIMARY_TEXT: Color = Color::Rgb(192, 202, 245);
pub(super) const FOCUS: Color = Color::Rgb(122, 162, 247);
pub(super) const INFO: Color = Color::Rgb(125, 207, 255);
pub(super) const SUCCESS: Color = Color::Rgb(158, 206, 106);
pub(super) const WARNING: Color = Color::Rgb(224, 175, 104);
pub(super) const ERROR: Color = Color::Rgb(247, 118, 142);

pub(super) const APPLICATION_COLORS: [Color; 16] = [
    Color::Rgb(122, 162, 247),
    Color::Rgb(125, 207, 255),
    Color::Rgb(115, 218, 202),
    Color::Rgb(42, 195, 222),
    Color::Rgb(158, 206, 106),
    Color::Rgb(224, 175, 104),
    Color::Rgb(255, 158, 100),
    Color::Rgb(247, 118, 142),
    Color::Rgb(187, 154, 247),
    Color::Rgb(157, 124, 216),
    Color::Rgb(137, 221, 255),
    Color::Rgb(13, 185, 215),
    Color::Rgb(65, 166, 181),
    Color::Rgb(219, 75, 75),
    Color::Rgb(255, 0, 124),
    Color::Rgb(26, 188, 156),
];
