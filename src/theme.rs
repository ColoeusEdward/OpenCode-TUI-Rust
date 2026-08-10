use anyhow::{Context, Result, anyhow, bail};
use ratatui::style::Color;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_THEME_NAME: &str = "material";
pub const CUSTOM_THEME_NAME: &str = "custom";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeMode {
    #[default]
    Dark,
    Light,
}

/// Semantic colors shared by every TUI surface.
///
/// The field names mirror the color roles in OpenCode's theme schema. Keeping
/// the roles here means widgets do not need to know which concrete palette is
/// active.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    pub name: &'static str,
    pub mode: ThemeMode,
    pub primary: Color,
    pub secondary: Color,
    pub accent: Color,
    pub error: Color,
    pub warning: Color,
    pub success: Color,
    pub info: Color,
    pub text: Color,
    pub text_muted: Color,
    pub selected_list_item_text: Color,
    pub background: Color,
    pub background_panel: Color,
    pub background_element: Color,
    pub background_menu: Color,
    pub border: Color,
    pub border_active: Color,
    pub border_subtle: Color,
    /// Border of the prompt box. Separate from `border_active` so the prompt can
    /// be tinted without also retinting every dialog border.
    pub prompt_border: Color,
    /// Background of the cell the prompt cursor occupies.
    pub prompt_cursor: Color,
    /// Background of mouse-selected text. Kept distinct from
    /// `background_element` so a selection is legible on top of rows that already
    /// use the element background.
    pub selection_background: Color,
    /// Foreground of mouse-selected text.
    pub selection_text: Color,
    pub diff_added: Color,
    pub diff_removed: Color,
    pub diff_context: Color,
    pub diff_hunk_header: Color,
    pub diff_highlight_added: Color,
    pub diff_highlight_removed: Color,
    pub diff_added_bg: Color,
    pub diff_removed_bg: Color,
    pub diff_context_bg: Color,
    pub diff_line_number: Color,
    pub diff_added_line_number_bg: Color,
    pub diff_removed_line_number_bg: Color,
    pub markdown_text: Color,
    pub markdown_heading: Color,
    pub markdown_link: Color,
    pub markdown_link_text: Color,
    pub markdown_code: Color,
    pub markdown_block_quote: Color,
    pub markdown_emph: Color,
    pub markdown_strong: Color,
    pub markdown_horizontal_rule: Color,
    pub markdown_list_item: Color,
    pub markdown_list_enumeration: Color,
    pub markdown_image: Color,
    pub markdown_image_text: Color,
    pub markdown_code_block: Color,
    pub syntax_comment: Color,
    pub syntax_keyword: Color,
    pub syntax_function: Color,
    pub syntax_variable: Color,
    pub syntax_string: Color,
    pub syntax_number: Color,
    pub syntax_type: Color,
    pub syntax_operator: Color,
    pub syntax_punctuation: Color,
    pub thinking_opacity: f32,
}

#[derive(Debug, Clone)]
pub struct ThemeChoice {
    pub name: String,
    pub spec: String,
    pub theme: Theme,
}

impl Default for Theme {
    fn default() -> Self {
        Self::material(ThemeMode::Dark)
    }
}

impl Theme {
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "material" | "material-dark" => Some(Self::material(ThemeMode::Dark)),
            "material-light" => Some(Self::material(ThemeMode::Light)),
            _ => None,
        }
    }

    pub fn choices() -> Vec<ThemeChoice> {
        Self::choices_in(Path::new("themes"))
    }

    fn choices_in(directory: &Path) -> Vec<ThemeChoice> {
        let mut choices = ["material", "material-light"]
            .into_iter()
            .filter_map(|name| {
                Self::named(name).map(|theme| ThemeChoice {
                    name: name.to_owned(),
                    spec: name.to_owned(),
                    theme,
                })
            })
            .collect::<Vec<_>>();
        let mut paths = fs::read_dir(directory)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| {
                path.is_file()
                    && path
                        .extension()
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
            })
            .collect::<Vec<_>>();
        paths.sort();
        for path in paths {
            let Some(name) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            if choices.iter().any(|choice| choice.name == name) {
                continue;
            }
            let Ok(theme) = Self::from_path(&path) else {
                continue;
            };
            choices.push(ThemeChoice {
                name: name.to_owned(),
                spec: path.to_string_lossy().into_owned(),
                theme,
            });
        }
        choices
    }

    pub fn load(spec: &str) -> Result<Self> {
        if let Some(theme) = Self::named(spec) {
            return Ok(theme);
        }
        let path = discover_theme_path(spec).ok_or_else(|| {
            anyhow!("theme {spec:?} was not found as a built-in theme or JSON file")
        })?;
        Self::from_path(path)
    }

    pub fn from_path(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let content = fs::read_to_string(path)
            .with_context(|| format!("failed to read theme file {}", path.display()))?;
        let document: ThemeDocument = serde_json::from_str(&content)
            .with_context(|| format!("failed to parse theme file {}", path.display()))?;
        let ThemeDocument {
            mode: mode_name,
            defs,
            theme: nested_overrides,
            overrides,
        } = document;
        let overrides = nested_overrides.unwrap_or(overrides);
        let mode = match mode_name.as_deref() {
            None => ThemeMode::Dark,
            Some("dark") => ThemeMode::Dark,
            Some("light") => ThemeMode::Light,
            Some(value) => bail!("invalid theme mode {value:?}; expected dark or light"),
        };
        let mut theme = Self::material(mode);
        theme.name = CUSTOM_THEME_NAME;
        apply_overrides(&mut theme, &overrides, &defs)?;
        Ok(theme)
    }

    pub fn material(mode: ThemeMode) -> Self {
        let dark = mode == ThemeMode::Dark;
        let pick = |dark_value: Color, light_value: Color| {
            if dark { dark_value } else { light_value }
        };

        let background = pick(rgb(38, 50, 56), rgb(250, 250, 250));
        let background_panel = pick(rgb(30, 39, 44), rgb(245, 245, 245));
        let background_element = pick(rgb(55, 71, 79), rgb(231, 231, 232));
        let text = pick(rgb(238, 255, 255), rgb(38, 50, 56));
        let text_muted = pick(rgb(84, 110, 122), rgb(144, 164, 174));
        let primary = pick(rgb(130, 170, 255), rgb(97, 130, 184));
        let secondary = pick(rgb(199, 146, 234), rgb(124, 77, 255));
        let accent = pick(rgb(137, 221, 255), rgb(57, 173, 181));
        let error = pick(rgb(240, 113, 120), rgb(229, 57, 53));
        let warning = pick(rgb(255, 203, 107), rgb(255, 179, 0));
        let success = pick(rgb(195, 232, 141), rgb(145, 184, 89));
        let info = pick(rgb(255, 203, 107), rgb(244, 81, 30));
        let code_comment = text_muted;
        let code_string = success;
        let code_number = info;

        Self {
            name: DEFAULT_THEME_NAME,
            mode,
            primary,
            secondary,
            accent,
            error,
            warning,
            success,
            info,
            text,
            text_muted,
            selected_list_item_text: background,
            background,
            background_panel,
            background_element,
            background_menu: background_element,
            border: pick(rgb(55, 71, 79), rgb(224, 224, 224)),
            border_active: primary,
            border_subtle: pick(rgb(30, 39, 44), rgb(238, 238, 238)),
            // Material's `secondary` is the palette's purple in both modes.
            prompt_border: secondary,
            prompt_cursor: secondary,
            // A muted fill rather than the accent purple: a selection can cover
            // many rows, and the prompt cursor should stay distinguishable from it.
            selection_background: pick(rgb(69, 90, 100), rgb(197, 202, 233)),
            selection_text: text,
            diff_added: success,
            diff_removed: error,
            diff_context: text_muted,
            diff_hunk_header: accent,
            diff_highlight_added: success,
            diff_highlight_removed: error,
            diff_added_bg: pick(rgb(46, 60, 43), rgb(232, 245, 233)),
            diff_removed_bg: pick(rgb(60, 43, 43), rgb(255, 235, 238)),
            diff_context_bg: background_panel,
            diff_line_number: pick(rgb(154, 162, 166), rgb(106, 110, 112)),
            diff_added_line_number_bg: pick(rgb(46, 60, 43), rgb(232, 245, 233)),
            diff_removed_line_number_bg: pick(rgb(60, 43, 43), rgb(255, 235, 238)),
            markdown_text: text,
            markdown_heading: primary,
            markdown_link: accent,
            markdown_link_text: secondary,
            markdown_code: success,
            markdown_block_quote: text_muted,
            markdown_emph: warning,
            markdown_strong: info,
            markdown_horizontal_rule: pick(rgb(55, 71, 79), rgb(224, 224, 224)),
            markdown_list_item: primary,
            markdown_list_enumeration: accent,
            markdown_image: accent,
            markdown_image_text: secondary,
            markdown_code_block: text,
            syntax_comment: code_comment,
            syntax_keyword: secondary,
            syntax_function: primary,
            syntax_variable: text,
            syntax_string: code_string,
            syntax_number: code_number,
            syntax_type: warning,
            syntax_operator: accent,
            syntax_punctuation: text,
            thinking_opacity: 0.6,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct ThemeDocument {
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    defs: HashMap<String, ColorSpec>,
    #[serde(default)]
    theme: Option<ThemeOverrides>,
    #[serde(flatten, default)]
    overrides: ThemeOverrides,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThemeOverrides {
    primary: Option<ColorSpec>,
    secondary: Option<ColorSpec>,
    accent: Option<ColorSpec>,
    error: Option<ColorSpec>,
    warning: Option<ColorSpec>,
    success: Option<ColorSpec>,
    info: Option<ColorSpec>,
    text: Option<ColorSpec>,
    text_muted: Option<ColorSpec>,
    selected_list_item_text: Option<ColorSpec>,
    background: Option<ColorSpec>,
    background_panel: Option<ColorSpec>,
    background_element: Option<ColorSpec>,
    background_menu: Option<ColorSpec>,
    border: Option<ColorSpec>,
    border_active: Option<ColorSpec>,
    border_subtle: Option<ColorSpec>,
    prompt_border: Option<ColorSpec>,
    prompt_cursor: Option<ColorSpec>,
    selection_background: Option<ColorSpec>,
    selection_text: Option<ColorSpec>,
    diff_added: Option<ColorSpec>,
    diff_removed: Option<ColorSpec>,
    diff_context: Option<ColorSpec>,
    diff_hunk_header: Option<ColorSpec>,
    diff_highlight_added: Option<ColorSpec>,
    diff_highlight_removed: Option<ColorSpec>,
    diff_added_bg: Option<ColorSpec>,
    diff_removed_bg: Option<ColorSpec>,
    diff_context_bg: Option<ColorSpec>,
    diff_line_number: Option<ColorSpec>,
    diff_added_line_number_bg: Option<ColorSpec>,
    diff_removed_line_number_bg: Option<ColorSpec>,
    markdown_text: Option<ColorSpec>,
    markdown_heading: Option<ColorSpec>,
    markdown_link: Option<ColorSpec>,
    markdown_link_text: Option<ColorSpec>,
    markdown_code: Option<ColorSpec>,
    markdown_block_quote: Option<ColorSpec>,
    markdown_emph: Option<ColorSpec>,
    markdown_strong: Option<ColorSpec>,
    markdown_horizontal_rule: Option<ColorSpec>,
    markdown_list_item: Option<ColorSpec>,
    markdown_list_enumeration: Option<ColorSpec>,
    markdown_image: Option<ColorSpec>,
    markdown_image_text: Option<ColorSpec>,
    markdown_code_block: Option<ColorSpec>,
    syntax_comment: Option<ColorSpec>,
    syntax_keyword: Option<ColorSpec>,
    syntax_function: Option<ColorSpec>,
    syntax_variable: Option<ColorSpec>,
    syntax_string: Option<ColorSpec>,
    syntax_number: Option<ColorSpec>,
    syntax_type: Option<ColorSpec>,
    syntax_operator: Option<ColorSpec>,
    syntax_punctuation: Option<ColorSpec>,
    thinking_opacity: Option<f32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum ColorSpec {
    Variant {
        dark: Box<ColorSpec>,
        light: Box<ColorSpec>,
    },
    Indexed(u16),
    String(String),
}

fn discover_theme_path(spec: &str) -> Option<PathBuf> {
    let requested = PathBuf::from(spec);
    [
        requested.clone(),
        PathBuf::from("themes").join(&requested),
        PathBuf::from("themes").join(format!("{spec}.json")),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

fn apply_overrides(
    theme: &mut Theme,
    overrides: &ThemeOverrides,
    defs: &HashMap<String, ColorSpec>,
) -> Result<()> {
    let roles = role_specs(overrides);
    macro_rules! apply_color {
        ($field:ident) => {
            if let Some(value) = overrides.$field.as_ref() {
                theme.$field = resolve_color(value, defs, &roles, theme.mode, &mut Vec::new())
                    .with_context(|| format!("invalid color for {}", stringify!($field)))?;
            }
        };
    }

    apply_color!(primary);
    apply_color!(secondary);
    apply_color!(accent);
    apply_color!(error);
    apply_color!(warning);
    apply_color!(success);
    apply_color!(info);
    apply_color!(text);
    apply_color!(text_muted);
    apply_color!(selected_list_item_text);
    apply_color!(background);
    apply_color!(background_panel);
    apply_color!(background_element);
    apply_color!(background_menu);
    apply_color!(border);
    apply_color!(border_active);
    apply_color!(border_subtle);
    apply_color!(prompt_border);
    apply_color!(prompt_cursor);
    apply_color!(selection_background);
    apply_color!(selection_text);
    apply_color!(diff_added);
    apply_color!(diff_removed);
    apply_color!(diff_context);
    apply_color!(diff_hunk_header);
    apply_color!(diff_highlight_added);
    apply_color!(diff_highlight_removed);
    apply_color!(diff_added_bg);
    apply_color!(diff_removed_bg);
    apply_color!(diff_context_bg);
    apply_color!(diff_line_number);
    apply_color!(diff_added_line_number_bg);
    apply_color!(diff_removed_line_number_bg);
    apply_color!(markdown_text);
    apply_color!(markdown_heading);
    apply_color!(markdown_link);
    apply_color!(markdown_link_text);
    apply_color!(markdown_code);
    apply_color!(markdown_block_quote);
    apply_color!(markdown_emph);
    apply_color!(markdown_strong);
    apply_color!(markdown_horizontal_rule);
    apply_color!(markdown_list_item);
    apply_color!(markdown_list_enumeration);
    apply_color!(markdown_image);
    apply_color!(markdown_image_text);
    apply_color!(markdown_code_block);
    apply_color!(syntax_comment);
    apply_color!(syntax_keyword);
    apply_color!(syntax_function);
    apply_color!(syntax_variable);
    apply_color!(syntax_string);
    apply_color!(syntax_number);
    apply_color!(syntax_type);
    apply_color!(syntax_operator);
    apply_color!(syntax_punctuation);

    if let Some(opacity) = overrides.thinking_opacity {
        if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
            bail!("thinking_opacity must be between 0 and 1");
        }
        theme.thinking_opacity = opacity;
    }
    Ok(())
}

fn role_specs(overrides: &ThemeOverrides) -> HashMap<String, &ColorSpec> {
    let mut roles = HashMap::new();
    macro_rules! insert_role {
        ($field:ident) => {
            if let Some(value) = overrides.$field.as_ref() {
                roles.insert(stringify!($field).to_owned(), value);
            }
        };
    }

    insert_role!(primary);
    insert_role!(secondary);
    insert_role!(accent);
    insert_role!(error);
    insert_role!(warning);
    insert_role!(success);
    insert_role!(info);
    insert_role!(text);
    insert_role!(text_muted);
    insert_role!(selected_list_item_text);
    insert_role!(background);
    insert_role!(background_panel);
    insert_role!(background_element);
    insert_role!(background_menu);
    insert_role!(border);
    insert_role!(border_active);
    insert_role!(border_subtle);
    insert_role!(prompt_border);
    insert_role!(prompt_cursor);
    insert_role!(selection_background);
    insert_role!(selection_text);
    insert_role!(diff_added);
    insert_role!(diff_removed);
    insert_role!(diff_context);
    insert_role!(diff_hunk_header);
    insert_role!(diff_highlight_added);
    insert_role!(diff_highlight_removed);
    insert_role!(diff_added_bg);
    insert_role!(diff_removed_bg);
    insert_role!(diff_context_bg);
    insert_role!(diff_line_number);
    insert_role!(diff_added_line_number_bg);
    insert_role!(diff_removed_line_number_bg);
    insert_role!(markdown_text);
    insert_role!(markdown_heading);
    insert_role!(markdown_link);
    insert_role!(markdown_link_text);
    insert_role!(markdown_code);
    insert_role!(markdown_block_quote);
    insert_role!(markdown_emph);
    insert_role!(markdown_strong);
    insert_role!(markdown_horizontal_rule);
    insert_role!(markdown_list_item);
    insert_role!(markdown_list_enumeration);
    insert_role!(markdown_image);
    insert_role!(markdown_image_text);
    insert_role!(markdown_code_block);
    insert_role!(syntax_comment);
    insert_role!(syntax_keyword);
    insert_role!(syntax_function);
    insert_role!(syntax_variable);
    insert_role!(syntax_string);
    insert_role!(syntax_number);
    insert_role!(syntax_type);
    insert_role!(syntax_operator);
    insert_role!(syntax_punctuation);
    roles
}

fn resolve_color(
    value: &ColorSpec,
    defs: &HashMap<String, ColorSpec>,
    roles: &HashMap<String, &ColorSpec>,
    mode: ThemeMode,
    chain: &mut Vec<String>,
) -> Result<Color> {
    match value {
        ColorSpec::Indexed(index) => {
            if *index > u8::MAX as u16 {
                bail!("ANSI color index must be between 0 and 255");
            }
            Ok(Color::Indexed(*index as u8))
        }
        ColorSpec::Variant { dark, light } => resolve_color(
            if mode == ThemeMode::Dark { dark } else { light },
            defs,
            roles,
            mode,
            chain,
        ),
        ColorSpec::String(value) => {
            let value = value.trim();
            let normalized = value.to_ascii_lowercase();
            if normalized == "transparent" || normalized == "none" {
                return Ok(Color::Reset);
            }
            if normalized.starts_with('#') || normalized.starts_with("0x") {
                return parse_color(value);
            }
            if let Some(color) = named_color(&normalized) {
                return Ok(color);
            }
            if chain.iter().any(|item| item == value) {
                bail!("circular color reference: {}", chain.join(" -> "));
            }
            chain.push(value.to_owned());
            let result = if let Some(definition) = defs.get(value) {
                resolve_color(definition, defs, roles, mode, chain)
            } else if let Some(role) = roles.get(&camel_to_snake(value)) {
                resolve_color(role, defs, roles, mode, chain)
            } else {
                bail!("color reference {value:?} was not found")
            };
            chain.pop();
            result
        }
    }
}

fn camel_to_snake(value: &str) -> String {
    value
        .chars()
        .enumerate()
        .flat_map(|(index, character)| {
            if character.is_uppercase() && index > 0 {
                vec!['_', character.to_ascii_lowercase()]
            } else {
                vec![character.to_ascii_lowercase()]
            }
        })
        .collect()
}

fn parse_color(value: &str) -> Result<Color> {
    let value = value.trim().to_ascii_lowercase();
    if let Some(color) = named_color(&value) {
        return Ok(color);
    }
    let [red, green, blue] = parse_hex(&value)?;
    Ok(Color::Rgb(red, green, blue))
}

fn named_color(value: &str) -> Option<Color> {
    Some(match value {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "gray" | "grey" => Color::Gray,
        "darkgray" | "dark_gray" => Color::DarkGray,
        "lightred" | "light_red" => Color::LightRed,
        "lightgreen" | "light_green" => Color::LightGreen,
        "lightyellow" | "light_yellow" => Color::LightYellow,
        "lightblue" | "light_blue" => Color::LightBlue,
        "lightmagenta" | "light_magenta" => Color::LightMagenta,
        "lightcyan" | "light_cyan" => Color::LightCyan,
        "white" => Color::White,
        "reset" => Color::Reset,
        _ => return None,
    })
}

fn parse_hex(value: &str) -> Result<[u8; 3]> {
    let digits = value
        .strip_prefix('#')
        .or_else(|| value.strip_prefix("0x"))
        .ok_or_else(|| anyhow!("expected a named color or #RRGGBB value"))?;
    let digits = if digits.len() == 3 || digits.len() == 4 {
        digits
            .chars()
            .take(3)
            .flat_map(|digit| [digit, digit])
            .collect::<String>()
    } else if digits.len() == 8 {
        digits[..6].to_owned()
    } else {
        digits.to_owned()
    };
    if digits.len() != 6 {
        bail!("expected a 3- or 6-digit hexadecimal color");
    }
    Ok([
        u8::from_str_radix(&digits[0..2], 16).context("invalid red channel")?,
        u8::from_str_radix(&digits[2..4], 16).context("invalid green channel")?,
        u8::from_str_radix(&digits[4..6], 16).context("invalid blue channel")?,
    ])
}

fn rgb(red: u8, green: u8, blue: u8) -> Color {
    Color::Rgb(red, green, blue)
}

#[cfg(test)]
mod tests {
    use super::{CUSTOM_THEME_NAME, DEFAULT_THEME_NAME, Theme, ThemeMode};
    use ratatui::style::Color;
    use std::fs;

    #[test]
    fn default_theme_is_material_dark() {
        let theme = Theme::default();

        assert_eq!(theme.name, DEFAULT_THEME_NAME);
        assert_eq!(theme.mode, ThemeMode::Dark);
        assert_eq!(theme.background, Color::Rgb(38, 50, 56));
        assert_eq!(theme.primary, Color::Rgb(130, 170, 255));
    }

    #[test]
    fn material_light_matches_original_roles() {
        let theme = Theme::named("material-light").expect("material light theme");

        assert_eq!(theme.mode, ThemeMode::Light);
        assert_eq!(theme.background, Color::Rgb(250, 250, 250));
        assert_eq!(theme.text, Color::Rgb(38, 50, 56));
        assert_eq!(theme.markdown_heading, Color::Rgb(97, 130, 184));
        assert_eq!(Theme::material(ThemeMode::Light), theme);
    }

    #[test]
    fn prompt_roles_are_purple_and_independent_of_dialog_borders() {
        for mode in [ThemeMode::Dark, ThemeMode::Light] {
            let theme = Theme::material(mode);

            assert_eq!(theme.prompt_border, theme.secondary);
            assert_eq!(theme.prompt_cursor, theme.secondary);
            // The prompt must not inherit the blue used by dialog borders, or
            // retinting it would silently retint every dialog too.
            assert_ne!(theme.prompt_border, theme.border_active);

            let Color::Rgb(red, green, blue) = theme.prompt_border else {
                panic!("material prompt border should be an RGB colour");
            };
            assert!(
                blue > green && red > green,
                "expected a purple prompt border, got rgb({red}, {green}, {blue})"
            );
        }
    }

    #[test]
    fn custom_themes_can_override_the_prompt_roles() {
        let path = std::env::temp_dir().join(format!(
            "opencode-tui-rust-prompt-theme-{}.json",
            std::process::id()
        ));
        fs::write(
            &path,
            r##"{"theme":{"promptBorder":"#ff00ff","promptCursor":"promptBorder"}}"##,
        )
        .expect("theme fixture should be written");

        let theme = Theme::from_path(&path).expect("custom theme should load");

        assert_eq!(theme.prompt_border, Color::Rgb(255, 0, 255));
        assert_eq!(theme.prompt_cursor, Color::Rgb(255, 0, 255));
        fs::remove_file(path).expect("theme fixture should be removed");
    }

    #[test]
    fn unknown_theme_names_are_rejected() {
        assert!(Theme::named("opencode").is_none());
    }

    #[test]
    fn custom_theme_file_overrides_roles_and_keeps_material_defaults() {
        let path = std::env::temp_dir().join(format!(
            "opencode-tui-rust-theme-{}.json",
            std::process::id()
        ));
        fs::write(
            &path,
            r##"{
                "mode": "light",
                "defs": {"muted": "#123456"},
                "theme": {
                    "background": "#101820",
                    "primary": {"dark": "#abc", "light": "#def"},
                    "textMuted": "muted",
                    "thinkingOpacity": 0.4
                }
            }"##,
        )
        .expect("theme fixture should be written");

        let theme = Theme::load(path.to_str().expect("theme path should be UTF-8"))
            .expect("custom theme should load");

        assert_eq!(theme.name, CUSTOM_THEME_NAME);
        assert_eq!(theme.mode, ThemeMode::Light);
        assert_eq!(theme.background, Color::Rgb(16, 24, 32));
        assert_eq!(theme.primary, Color::Rgb(221, 238, 255));
        assert_eq!(theme.text_muted, Color::Rgb(18, 52, 86));
        assert_eq!(theme.thinking_opacity, 0.4);
        assert_eq!(theme.text, Color::Rgb(38, 50, 56));
        fs::remove_file(path).expect("theme fixture should be removed");
    }

    #[test]
    fn invalid_custom_theme_color_is_rejected() {
        let path = std::env::temp_dir().join(format!(
            "opencode-tui-rust-invalid-theme-{}.json",
            std::process::id()
        ));
        fs::write(&path, r##"{"background":"not-a-color"}"##)
            .expect("theme fixture should be written");

        assert!(Theme::from_path(&path).is_err());
        fs::remove_file(path).expect("theme fixture should be removed");
    }

    #[test]
    fn choices_include_valid_json_files_and_skip_invalid_files() {
        let directory =
            std::env::temp_dir().join(format!("opencode-tui-rust-themes-{}", std::process::id()));
        fs::create_dir_all(&directory).expect("theme directory should be created");
        fs::write(
            directory.join("forest.json"),
            r##"{"theme":{"background":"#112233"}}"##,
        )
        .expect("valid theme fixture should be written");
        fs::write(directory.join("broken.json"), "not json")
            .expect("invalid theme fixture should be written");

        let choices = Theme::choices_in(&directory);

        assert_eq!(choices.len(), 3);
        assert_eq!(choices[2].name, "forest");
        assert_eq!(choices[2].theme.background, Color::Rgb(17, 34, 51));
        fs::remove_dir_all(directory).expect("theme directory should be removed");
    }
}
