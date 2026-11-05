//! Reads the live Omarchy theme and turns it into GTK CSS.
//!
//! Sources, in the same way the Omarchy shell reads them:
//! - `current/theme/colors.toml` for the palette
//! - `current/theme/shell.toml` for control states, spacing and font size
//! - `~/.config/omarchy/shell.toml` `[beacon]` for user overrides
//! - `hyprctl getoption decoration:rounding` for corner radius
//! - fontconfig's monospace family, which is what `omarchy font set` changes

use std::process::Command;
use toml::{Table, Value};

use crate::paths;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba {
    r: u8,
    g: u8,
    b: u8,
    a: f64,
}

impl Rgba {
    const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Rgba { r, g, b, a: 1.0 }
    }

    pub fn alpha(self, a: f64) -> Self {
        Rgba {
            a: (self.a * a).clamp(0.0, 1.0),
            ..self
        }
    }

    fn css(self) -> String {
        format!("rgba({}, {}, {}, {:.3})", self.r, self.g, self.b, self.a)
    }

    /// Accepts `#rgb`, `#rrggbb`, `#rrggbbaa`, Hyprland `rgb(rrggbb)` /
    /// `rgba(rrggbbaa)` and CSS `rgb(r, g, b)` / `rgba(r, g, b, a)`. For a
    /// Hyprland gradient (`rgba(..) rgba(..) 45deg`) the first stop is used.
    pub fn parse(raw: &str) -> Option<Self> {
        let s = raw.trim();
        if let Some(hex) = s.strip_prefix('#') {
            return parse_hex(hex);
        }
        let lower = s.to_ascii_lowercase();
        let (inner, _) = lower
            .strip_prefix("rgba(")
            .or_else(|| lower.strip_prefix("rgb("))?
            .split_once(')')?;
        if !inner.contains(',') {
            return parse_hex(inner.trim());
        }
        let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
        if parts.len() < 3 {
            return None;
        }
        let r = parts[0].parse::<f64>().ok()?;
        let g = parts[1].parse::<f64>().ok()?;
        let b = parts[2].parse::<f64>().ok()?;
        let a = parts.get(3).and_then(|v| v.parse::<f64>().ok()).unwrap_or(1.0);
        Some(Rgba {
            r: r.clamp(0.0, 255.0) as u8,
            g: g.clamp(0.0, 255.0) as u8,
            b: b.clamp(0.0, 255.0) as u8,
            a: a.clamp(0.0, 1.0),
        })
    }
}

fn parse_hex(hex: &str) -> Option<Rgba> {
    let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    match hex.len() {
        3 => {
            let n = |i: usize| u8::from_str_radix(hex.get(i..i + 1)?, 16).ok().map(|v| v * 17);
            Some(Rgba::rgb(n(0)?, n(1)?, n(2)?))
        }
        6 => Some(Rgba::rgb(byte(0)?, byte(2)?, byte(4)?)),
        8 => Some(Rgba {
            r: byte(0)?,
            g: byte(2)?,
            b: byte(4)?,
            a: byte(6)? as f64 / 255.0,
        }),
        _ => None,
    }
}

/// One interactive state from shell.toml `[controls]`.
#[derive(Clone, Copy, Debug)]
struct State {
    fill: Rgba,
    border: Rgba,
    border_width: f64,
}

#[derive(Clone, Debug)]
pub struct Theme {
    background: Rgba,
    surface: Rgba,
    foreground: Rgba,
    muted: Rgba,
    accent: Rgba,
    urgent: Rgba,
    window_alpha: f64,
    normal: State,
    hover: State,
    focus: State,
    selected: State,
    pressed_fill: Rgba,
    selection_fill: Rgba,
    tooltip_bg: Rgba,
    tooltip_border: Rgba,
    font_family: String,
    base_size: f64,
    spacing: f64,
    radius: f64,
}

fn read_table(path: &std::path::Path) -> Table {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.parse::<Table>().ok())
        .unwrap_or_default()
}

fn get<'a>(t: &'a Table, section: &str, key: &str) -> Option<&'a Value> {
    t.get(section)?.as_table()?.get(key)
}

fn num(v: Option<&Value>) -> Option<f64> {
    match v? {
        Value::Float(f) => Some(*f),
        Value::Integer(i) => Some(*i as f64),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// A border width such as `1` or `"1 2"`; Beacon draws uniform borders, so
/// the first value is used.
fn width(v: Option<&Value>) -> Option<f64> {
    match v? {
        Value::String(s) => s.split_whitespace().next()?.parse().ok(),
        other => num(Some(other)),
    }
}

impl Theme {
    pub fn load() -> Theme {
        let theme_dir = paths::omarchy_current().join("theme");
        let colors = read_table(&theme_dir.join("colors.toml"));
        let shell = read_table(&theme_dir.join("shell.toml"));
        let user_shell = read_table(&paths::omarchy_user_shell_toml());

        let color = |key: &str| colors.get(key).and_then(Value::as_str).and_then(Rgba::parse);

        let background = color("background").unwrap_or(Rgba::rgb(0x12, 0x12, 0x12));
        let foreground = color("foreground").unwrap_or(Rgba::rgb(0xe6, 0xe6, 0xe6));
        let accent = color("accent")
            .or_else(|| color("blue"))
            .unwrap_or(foreground);
        let muted = color("muted").unwrap_or(foreground.alpha(0.55));
        let urgent = color("red").unwrap_or(accent);
        let surface = color("lighter_background").unwrap_or(foreground.alpha(0.08));

        // `hyprland.active-border` style references resolve against [hyprland].
        let resolve = |raw: &str, fallback: Rgba| -> Rgba {
            let token = raw.trim();
            match token.to_ascii_lowercase().as_str() {
                "foreground" | "text" => return foreground,
                "accent" => return accent,
                "urgent" => return urgent,
                "background" => return background,
                "transparent" => return foreground.alpha(0.0),
                _ => {}
            }
            if let Some(key) = token.strip_prefix("hyprland.") {
                return get(&shell, "hyprland", key)
                    .and_then(Value::as_str)
                    .and_then(Rgba::parse)
                    .unwrap_or(fallback);
            }
            Rgba::parse(token).unwrap_or(fallback)
        };

        let state = |name: &str, fill_alpha: f64, border_alpha: f64, border_width: f64| {
            let c = |k: &str| get(&shell, "controls", &format!("{name}-{k}"));
            let base = c("color")
                .and_then(Value::as_str)
                .map(|s| resolve(s, foreground))
                .unwrap_or(foreground);
            let border = c("border")
                .and_then(Value::as_str)
                .map(|s| resolve(s, base))
                .unwrap_or(base);
            State {
                fill: base.alpha(num(c("fill-alpha")).unwrap_or(fill_alpha)),
                border: border.alpha(num(c("border-alpha")).unwrap_or(border_alpha)),
                border_width: width(c("border-width")).unwrap_or(border_width),
            }
        };

        let normal = state("normal", 0.04, 0.4, 1.0);
        let hover = state("hover-cursor", 0.08, 0.25, 1.0);
        let focus = state("focus", 0.08, 0.25, 1.0);
        let selected = state("selected", 0.18, 1.0, 0.0);
        let ctl = |k: &str, d: f64| num(get(&shell, "controls", k)).unwrap_or(d);
        let pressed_fill = foreground.alpha(ctl("pressed-fill-alpha", 0.22));
        let selection_fill = accent.alpha(ctl("selection-fill-alpha", 0.35));

        let tooltip_bg = get(&shell, "tooltip", "background")
            .and_then(Value::as_str)
            .map(|s| resolve(s, background))
            .unwrap_or(background)
            .alpha(num(get(&shell, "tooltip", "background-alpha")).unwrap_or(0.97));
        let tooltip_border = get(&shell, "tooltip", "border")
            .and_then(Value::as_str)
            .map(|s| resolve(s, foreground))
            .unwrap_or(foreground);

        // [beacon] background-alpha: the user-wide file wins over the theme.
        let window_alpha = num(get(&user_shell, "beacon", "background-alpha"))
            .or_else(|| num(get(&shell, "beacon", "background-alpha")))
            .unwrap_or(1.0)
            .clamp(0.0, 1.0);

        let base_size = num(get(&shell, "font", "base-size"))
            .filter(|v| *v >= 1.0)
            .unwrap_or(12.0);
        let scale = num(get(&shell, "spacing", "scale")).unwrap_or(1.0).max(0.25);
        let scale_with_font = get(&shell, "spacing", "scale-with-font")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let spacing = scale * if scale_with_font { base_size / 12.0 } else { 1.0 };

        Theme {
            background,
            surface,
            foreground,
            muted,
            accent,
            urgent,
            window_alpha,
            normal,
            hover,
            focus,
            selected,
            pressed_fill,
            selection_fill,
            tooltip_bg,
            tooltip_border,
            font_family: font_family(),
            base_size,
            spacing,
            radius: hyprland_rounding(),
        }
    }

    fn sp(&self, px: f64) -> String {
        format!("{}px", (px * self.spacing).round().max(0.0))
    }

    fn fs(&self, factor: f64) -> String {
        format!("{:.1}px", self.base_size * factor)
    }

    pub fn css(&self) -> String {
        let bg = self.background.alpha(self.window_alpha).css();
        let fg = self.foreground.css();
        let muted = self.muted.css();
        let faint = self.foreground.alpha(0.35).css();
        let accent = self.accent.css();
        let urgent = self.urgent.css();
        let solid_bg = self.background.css();
        let surface = self.surface.css();
        let r = format!("{}px", self.radius);
        // Toggle and checkbox go pill-shaped only when Hyprland rounds corners,
        // the same rule as the shell's ToggleSwitch.
        let pill = if self.radius > 0.0 { "999px" } else { "0px" };
        let n = self.normal;
        let h = self.hover;
        let f = self.focus;
        let s = self.selected;
        let bw = |st: State| format!("{}px", st.border_width.max(0.0));
        let selected_fill = self.foreground.alpha(s.fill.a).css();
        let track = self.foreground.alpha(s.fill.a).css();
        let knob = format!("{}px", (14.0 * self.spacing).round().max(10.0));
        let knob_margin = format!("-{}px", ((14.0 * self.spacing).round().max(10.0) - 4.0) / 2.0);
        let check = format!("{}px", (16.0 * self.spacing).round().max(12.0));
        let family = self.font_family.replace('"', "");

        format!(
            r#"
* {{
  box-shadow: none;
  text-shadow: none;
  -gtk-icon-shadow: none;
  outline: none;
}}

window.beacon,
window.beacon.background {{
  background-color: {bg};
  color: {fg};
  font-family: "{family}", monospace;
  font-size: {body};
}}

window.beacon label {{ color: inherit; }}

.beacon .app-title {{
  font-size: {display};
  font-weight: 700;
  letter-spacing: -0.02em;
}}
.beacon .status {{ color: {muted}; font-size: {caption}; }}
.beacon .muted {{ color: {muted}; }}
.beacon .hint {{ color: {faint}; font-size: {caption}; }}
.beacon .empty {{ color: {muted}; padding: {sp24} 0; }}
.beacon .numeric {{ font-feature-settings: "tnum"; }}
.beacon .paused {{ opacity: 0.5; }}

/* rows */
.beacon .reminder {{
  padding: {sp10} {sp12};
  border-radius: {r};
  border: {nbw} solid transparent;
  background-color: transparent;
  transition: background-color 120ms ease-out, border-color 120ms ease-out;
}}
.beacon .reminder.cursor {{
  background-color: {h_fill};
  border-color: {h_border};
}}
.beacon .reminder .name {{ font-size: {subtitle}; }}
.beacon .reminder .value {{ font-size: {body}; min-width: 4.5em; }}
.beacon .reminder .countdown {{ color: {muted}; font-size: {caption}; }}
.beacon .reminder.off .name,
.beacon .reminder.off .value {{ color: {muted}; }}

/* buttons */
.beacon button {{
  min-height: {sp28};
  min-width: {sp28};
  padding: 0 {sp10};
  border-radius: {r};
  border: {nbw} solid {n_border};
  background: {n_fill};
  color: {fg};
  transition: background-color 100ms ease-out, border-color 100ms ease-out;
}}
.beacon button:hover {{ background: {h_fill}; border-color: {h_border}; }}
.beacon button:focus-visible {{ background: {f_fill}; border: {fbw} solid {f_border}; }}
.beacon button:active,
.beacon button:checked {{ background: {pressed}; }}
.beacon button.icon {{ padding: 0; font-size: {icon}; }}
.beacon button.flat {{ background: transparent; border-color: transparent; }}
.beacon button.flat:hover {{ background: {h_fill}; border-color: {h_border}; }}
.beacon button.destructive:hover {{ color: {urgent}; border-color: {urgent}; }}
.beacon button.add {{
  min-height: {sp36};
  border-style: dashed;
  color: {muted};
}}
.beacon button.add:hover {{ color: {fg}; }}
.beacon button.editing {{ background: {selected_fill}; border-color: {s_border}; }}

/* text entry */
.beacon entry {{
  min-height: {sp28};
  padding: 0 {sp8};
  border-radius: {r};
  border: {nbw} solid {n_border};
  background: {n_fill};
  color: {fg};
  caret-color: {accent};
  font-size: {subtitle};
}}
.beacon entry:focus-within {{ background: {f_fill}; border: {fbw} solid {f_border}; }}
.beacon entry placeholder {{ color: {faint}; }}
.beacon entry selection,
.beacon text selection {{ background-color: {selection}; color: {fg}; }}

/* master switch */
.beacon switch {{
  min-width: {sp40};
  min-height: {sp22};
  padding: 2px;
  border-radius: {pill};
  border: {nbw} solid {n_border};
  background: {n_fill};
  color: transparent;
  transition: background-color 120ms ease-out;
}}
.beacon switch image {{ opacity: 0; }}
.beacon switch:hover {{ border-color: {h_border}; }}
.beacon switch:focus-visible {{ border: {fbw} solid {f_border}; }}
.beacon switch:checked {{ background: {accent}; border-color: {accent}; }}
.beacon switch slider {{
  min-width: {sp18};
  min-height: {sp18};
  margin: 0;
  border-radius: {pill};
  border: none;
  background: {muted};
}}
.beacon switch:checked slider {{ background: {solid_bg}; }}

/* checkbox */
.beacon checkbutton {{ padding: 0; min-height: 0; }}
.beacon checkbutton check {{
  min-width: {check};
  min-height: {check};
  margin: 0;
  border-radius: {r};
  border: {nbw} solid {n_border};
  background: {n_fill};
  color: transparent;
  -gtk-icon-size: {check};
}}
.beacon checkbutton check:hover {{ border-color: {fg}; }}
.beacon checkbutton check:checked {{
  background: {accent};
  border-color: {accent};
  color: {solid_bg};
}}
.beacon checkbutton:focus-visible check {{ border-color: {f_border}; }}
.beacon checkbutton label {{ padding-left: {sp8}; }}

/* slider, after the shell's PanelSlider: thin rounded track, foreground fill,
   round knob ringed in the background color */
.beacon scale {{ padding: {sp8} {sp4}; margin: 0; }}
.beacon scale trough {{
  min-height: 4px;
  min-width: 0;
  margin: 0;
  border-radius: 999px;
  border: none;
  background: {track};
}}
.beacon scale highlight {{
  min-height: 4px;
  min-width: 0;
  margin: 0;
  border-radius: 999px;
  border: none;
  background: {fg};
}}
.beacon scale slider {{
  min-width: {knob};
  min-height: {knob};
  margin: {knob_margin};
  border-radius: 999px;
  border: 2px solid {solid_bg};
  background: {fg};
  transition: transform 110ms ease-out;
}}
.beacon scale slider:hover {{ transform: scale(1.12); }}
.beacon scale:focus-visible slider {{ border-color: {accent}; }}
.beacon scale marks {{ color: {faint}; margin-top: 2px; }}
.beacon scale marks mark indicator {{
  min-width: 1px;
  min-height: 4px;
  background: {faint};
  color: {faint};
}}
.beacon .reminder.off scale highlight,
.beacon .reminder.off scale slider {{ background: {muted}; }}

/* scrollbars */
.beacon scrollbar {{ background: transparent; border: none; }}
.beacon scrollbar slider {{
  min-width: 3px;
  min-height: 3px;
  border-radius: 999px;
  background: {faint};
  border: none;
}}
.beacon undershoot, .beacon overshoot {{ background: none; }}

separator.beacon-sep {{ background: {n_border}; min-height: 1px; }}

/* entry context menu and tooltips */
popover.background > contents,
.beacon popover > contents {{
  background: {solid_bg};
  color: {fg};
  border: 1px solid {tooltip_border};
  border-radius: {r};
}}
popover modelbutton:hover {{ background: {h_fill}; }}
tooltip.background, tooltip {{
  background: {tooltip_bg};
  color: {fg};
  border: 1px solid {tooltip_border};
  border-radius: {r};
}}
.beacon .surface {{ background: {surface}; }}
"#,
            body = self.fs(1.0),
            caption = self.fs(0.833),
            subtitle = self.fs(1.083),
            display = self.fs(1.667),
            icon = self.fs(1.333),
            sp4 = self.sp(4.0),
            sp8 = self.sp(8.0),
            sp10 = self.sp(10.0),
            sp12 = self.sp(12.0),
            sp18 = self.sp(18.0),
            sp22 = self.sp(22.0),
            sp24 = self.sp(24.0),
            sp28 = self.sp(28.0),
            sp36 = self.sp(36.0),
            sp40 = self.sp(40.0),
            nbw = bw(n),
            fbw = bw(f),
            n_fill = n.fill.css(),
            n_border = n.border.css(),
            h_fill = h.fill.css(),
            h_border = h.border.css(),
            f_fill = f.fill.css(),
            f_border = f.border.css(),
            s_border = s.border.alpha(0.6).css(),
            pressed = self.pressed_fill.css(),
            selection = self.selection_fill.css(),
            tooltip_bg = self.tooltip_bg.css(),
            tooltip_border = self.tooltip_border.css(),
        )
    }

    /// Outer padding of the window content, scaled like the shell's panels.
    pub fn padding(&self) -> i32 {
        (18.0 * self.spacing).round() as i32
    }

    pub fn gap(&self, px: f64) -> i32 {
        (px * self.spacing).round() as i32
    }
}

fn font_family() -> String {
    Command::new("fc-match")
        .args(["monospace", "-f", "%{family}"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.split(',').next().map(|f| f.trim().to_string()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "monospace".into())
}

fn hyprland_rounding() -> f64 {
    let out = Command::new("hyprctl")
        .args(["getoption", "decoration:rounding", "-j"])
        .output();
    let Ok(out) = out else { return 0.0 };
    let text = String::from_utf8_lossy(&out.stdout);
    // {"option": "decoration:rounding", "int": 0, ...}
    text.split("\"int\"")
        .nth(1)
        .and_then(|rest| {
            let digits: String = rest
                .trim_start_matches([':', ' '])
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            digits.parse::<f64>().ok()
        })
        .unwrap_or(0.0)
        .min(24.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_colors() {
        assert_eq!(Rgba::parse("#ffffff"), Some(Rgba::rgb(255, 255, 255)));
        assert_eq!(Rgba::parse("#fff"), Some(Rgba::rgb(255, 255, 255)));
        let c = Rgba::parse("rgba(33ccffee) rgba(00ff99ee) 45deg").unwrap();
        assert_eq!((c.r, c.g, c.b), (0x33, 0xcc, 0xff));
        let c = Rgba::parse("rgba(10, 20, 30, 0.5)").unwrap();
        assert_eq!((c.r, c.g, c.b, c.a), (10, 20, 30, 0.5));
        assert_eq!(Rgba::parse("rgb(0a0b0c)"), Some(Rgba::rgb(10, 11, 12)));
        assert_eq!(Rgba::parse("nope"), None);
    }
}
