//! zellij-salto: salta entre los paneles de la pestaña actual con letras,
//! al estilo de nvim-window. Un solo .wasm con dos modos, según la
//! configuración `modo` del alias del plugin:
//!
//! - Modo "letras" (por defecto): plugin en segundo plano. Al activarlo con
//!   MessagePlugin "salto" renombra cada panel a "[letra] título", intercepta
//!   las teclas y enfoca el panel elegido. Necesita `pane_frames true`.
//! - Modo "mapa" (`modo "mapa"`): panel flotante a pantalla completa que
//!   dibuja los paneles de la pestaña con su letra. No toca los títulos ni
//!   intercepta el teclado, y funciona con `pane_frames false`.

use std::collections::BTreeMap;
use std::io::Write;
use zellij_tile::prelude::*;

/// Letras disponibles para las etiquetas (fila central primero).
/// Con más paneles que letras se usan etiquetas de dos letras.
const CHARS: &str = "asdfghjklqwertyuiopzxcvbnm";

/// Segundos antes de cancelar solos (modo letras), para no dejar el teclado
/// interceptado.
const TIMEOUT_SECS: f64 = 8.0;

/// Cómo restaurar los títulos al terminar (modo letras):
/// - false: se vuelve a poner el título que tenía (queda como nombre fijo).
/// - true: se renombra a "" para intentar que Zellij vuelva al título dinámico.
const RESTORE_WITH_EMPTY_NAME: bool = false;

// Bits para dibujar las líneas del mapa.
const U: u8 = 1;
const D: u8 = 2;
const L: u8 = 4;
const R: u8 = 8;
const BOX: [char; 16] = [
    ' ', '│', '│', '│', '─', '╯', '╮', '┤', '─', '╰', '╭', '├', '─', '┴', '┬', '┼',
];

#[derive(Default, PartialEq)]
enum Mode {
    #[default]
    Hints,
    Map,
}

struct Hint {
    label: String,
    pane_id: u32,
    original_title: String,
}

struct Picking {
    hints: Vec<Hint>,
    typed: String,
}

enum Outcome {
    Wait,
    Cancel,
    Jump(u32),
}

#[derive(Default)]
struct State {
    mode: Mode,
    granted: bool,
    active_tab: Option<usize>,
    manifest: Option<PaneManifest>,
    // Modo letras
    picking: Option<Picking>,
    // Modo mapa
    map_typed: String,
    theme: Theme,
}

register_plugin!(State);

impl ZellijPlugin for State {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        self.theme = Theme::from_config(&configuration);
        self.mode = match configuration.get("modo").map(|s| s.as_str()) {
            Some("mapa") => Mode::Map,
            _ => Mode::Hints,
        };

        let mut permissions = vec![
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
        ];
        let mut events = vec![
            EventType::PermissionRequestResult,
            EventType::TabUpdate,
            EventType::PaneUpdate,
        ];
        match self.mode {
            Mode::Hints => {
                permissions.push(PermissionType::InterceptInput);
                events.extend([
                    EventType::InterceptedKeyPress,
                    EventType::Timer,
                    EventType::BeforeClose,
                ]);
            }
            Mode::Map => events.push(EventType::Key),
        }
        request_permission(&permissions);
        subscribe(&events);
    }

    fn update(&mut self, event: Event) -> bool {
        let is_map = self.mode == Mode::Map;
        match event {
            Event::PermissionRequestResult(status) => {
                self.granted = status == PermissionStatus::Granted;
                if self.granted && is_map {
                    self.fullscreen_self();
                }
            }
            Event::TabUpdate(tabs) => {
                self.active_tab = tabs.iter().find(|t| t.active).map(|t| t.position);
                return is_map;
            }
            Event::PaneUpdate(manifest) => {
                self.manifest = Some(manifest);
                return is_map;
            }
            Event::Key(key) => {
                self.on_map_key(key);
                return true;
            }
            Event::InterceptedKeyPress(key) => self.on_key(key),
            Event::Timer(_) => self.finish(None),
            Event::BeforeClose => self.finish(None),
            _ => {}
        }
        false
    }

    fn pipe(&mut self, message: PipeMessage) -> bool {
        if message.name == "salto" {
            self.start();
        }
        false
    }

    fn render(&mut self, rows: usize, cols: usize) {
        if self.mode == Mode::Map {
            self.render_map(rows, cols);
        }
    }
}

impl State {
    /// Paneles de terminal visibles de la pestaña activa, en orden de lectura
    /// (de arriba abajo, de izquierda a derecha).
    fn current_panes(&self, include_floating: bool) -> Vec<&PaneInfo> {
        let (Some(tab), Some(manifest)) = (self.active_tab, self.manifest.as_ref()) else {
            return Vec::new();
        };
        let mut panes: Vec<&PaneInfo> = manifest
            .panes
            .get(&tab)
            .map(|v| {
                v.iter()
                    .filter(|p| {
                        !p.is_plugin
                            && !p.is_suppressed
                            && p.is_selectable
                            && (include_floating || !p.is_floating)
                    })
                    .collect()
            })
            .unwrap_or_default();
        panes.sort_by_key(|p| (p.pane_y, p.pane_x));
        panes.truncate(CHARS.chars().count().pow(2));
        panes
    }

    // ───────────────────────── Modo letras ─────────────────────────

    fn start(&mut self) {
        if self.mode != Mode::Hints || !self.granted || self.picking.is_some() {
            return;
        }
        let panes = self.current_panes(true);
        if panes.len() < 2 {
            return;
        }
        let chars: Vec<char> = CHARS.chars().collect();
        let labels = make_labels(panes.len(), &chars);
        let hints: Vec<Hint> = panes
            .iter()
            .zip(labels)
            .map(|(p, label)| Hint {
                label,
                pane_id: p.id,
                original_title: p.title.clone(),
            })
            .collect();

        for h in &hints {
            rename_terminal_pane(h.pane_id, &format!("[{}] {}", h.label, h.original_title));
        }

        intercept_key_presses();
        set_timeout(TIMEOUT_SECS);
        self.picking = Some(Picking {
            hints,
            typed: String::new(),
        });
    }

    fn on_key(&mut self, key: KeyWithModifier) {
        let outcome = match self.picking.as_mut() {
            None => return,
            Some(p) => match key.bare_key {
                BareKey::Char(c) if key.has_no_modifiers() => {
                    p.typed.push(c.to_ascii_lowercase());
                    let matches: Vec<&Hint> = p
                        .hints
                        .iter()
                        .filter(|h| h.label.starts_with(&p.typed))
                        .collect();
                    match matches.as_slice() {
                        [] => Outcome::Cancel,
                        [h] if h.label == p.typed => Outcome::Jump(h.pane_id),
                        _ => Outcome::Wait,
                    }
                }
                // Esc, Ctrl+c, Enter, etc.: cancelar.
                _ => Outcome::Cancel,
            },
        };

        match outcome {
            Outcome::Wait => {}
            Outcome::Cancel => self.finish(None),
            Outcome::Jump(id) => self.finish(Some(id)),
        }
    }

    /// Restaura títulos, libera el teclado y, si hay destino, enfoca ese panel.
    fn finish(&mut self, target: Option<u32>) {
        let Some(p) = self.picking.take() else {
            return;
        };
        for h in &p.hints {
            let name = if RESTORE_WITH_EMPTY_NAME {
                ""
            } else {
                h.original_title.as_str()
            };
            rename_terminal_pane(h.pane_id, name);
        }
        clear_key_presses_intercepts();
        if let Some(id) = target {
            // 0.45.1: (id, should_float_if_hidden, should_be_in_place_if_hidden)
            focus_terminal_pane(id, false, false);
        }
    }

    // ───────────────────────── Modo mapa ─────────────────────────

    /// Pide a Zellij que este panel flotante ocupe toda el área.
    fn fullscreen_self(&self) {
        let id = get_plugin_ids().plugin_id;
        // Se usan los métodos "with_*" para no depender del nombre del tipo
        // interno (PercentOrFixed / SplitSize), que cambia entre versiones.
        let coordinates = FloatingPaneCoordinates::default()
            .with_x_fixed(0)
            .with_y_fixed(0)
            .with_width_percent(100)
            .with_height_percent(100);
        change_floating_panes_coordinates(vec![(PaneId::Plugin(id), coordinates)]);
    }

    fn on_map_key(&mut self, key: KeyWithModifier) {
        if self.mode != Mode::Map {
            return;
        }
        let c = match key.bare_key {
            BareKey::Char(c) if key.has_no_modifiers() => c.to_ascii_lowercase(),
            // Esc, Ctrl+c, Enter, etc.: cancelar.
            _ => {
                close_self();
                return;
            }
        };
        self.map_typed.push(c);

        let outcome = {
            let panes = self.current_panes(false);
            let chars: Vec<char> = CHARS.chars().collect();
            let labels = make_labels(panes.len(), &chars);
            let matches: Vec<(&String, u32)> = labels
                .iter()
                .zip(panes.iter())
                .filter(|(l, _)| l.starts_with(&self.map_typed))
                .map(|(l, p)| (l, p.id))
                .collect();
            match matches.as_slice() {
                [] => Outcome::Cancel,
                [(l, id)] if l.as_str() == self.map_typed.as_str() => Outcome::Jump(*id),
                _ => Outcome::Wait,
            }
        };

        match outcome {
            Outcome::Wait => {}
            Outcome::Cancel => close_self(),
            Outcome::Jump(id) => {
                focus_terminal_pane(id, false, false);
                close_self();
            }
        }
    }

    fn render_map(&self, rows: usize, cols: usize) {
        let panes = self.current_panes(false);
        if panes.is_empty() || rows < 6 || cols < 12 {
            print!("\x1b[1;1HSin paneles que mostrar (Esc: cerrar)");
            std::io::stdout().flush().ok();
            return;
        }
        let t = &self.theme;
        let chars: Vec<char> = CHARS.chars().collect();
        let labels = make_labels(panes.len(), &chars);

        // Rectángulo que ocupan todos los paneles, para escalarlo al mapa.
        let min_x = panes.iter().map(|p| p.pane_x).min().unwrap_or(0);
        let min_y = panes.iter().map(|p| p.pane_y).min().unwrap_or(0);
        let max_x = panes
            .iter()
            .map(|p| p.pane_x + p.pane_columns)
            .max()
            .unwrap_or(1);
        let max_y = panes
            .iter()
            .map(|p| p.pane_y + p.pane_rows)
            .max()
            .unwrap_or(1);
        let span_x = (max_x - min_x).max(1) as f64;
        let span_y = (max_y - min_y).max(1) as f64;

        // La última fila se reserva para la barra inferior.
        let grid_rows = rows - 1;
        let grid_cols = cols;
        let sx = (grid_cols - 1) as f64 / span_x;
        let sy = (grid_rows - 1) as f64 / span_y;
        let ex = |x: usize| (((x - min_x) as f64) * sx).round() as usize;
        let ey = |y: usize| (((y - min_y) as f64) * sy).round() as usize;

        // Pasada 1: rectángulos y máscara de líneas. Los bordes compartidos
        // entre paneles vecinos coinciden y las uniones salen de la máscara.
        let mut mask = vec![vec![0u8; grid_cols]; grid_rows];
        let mut rects = Vec::new();
        for (pane, label) in panes.iter().zip(labels.iter()) {
            let x0 = ex(pane.pane_x).min(grid_cols - 1);
            let x1 = ex(pane.pane_x + pane.pane_columns).min(grid_cols - 1);
            let y0 = ey(pane.pane_y).min(grid_rows - 1);
            let y1 = ey(pane.pane_y + pane.pane_rows).min(grid_rows - 1);
            if x1 <= x0 || y1 <= y0 {
                continue;
            }
            for x in x0..=x1 {
                if x > x0 {
                    mask[y0][x] |= L;
                    mask[y1][x] |= L;
                }
                if x < x1 {
                    mask[y0][x] |= R;
                    mask[y1][x] |= R;
                }
            }
            for y in y0..=y1 {
                if y > y0 {
                    mask[y][x0] |= U;
                    mask[y][x1] |= U;
                }
                if y < y1 {
                    mask[y][x0] |= D;
                    mask[y][x1] |= D;
                }
            }
            rects.push((x0, y0, x1, y1, *pane, label));
        }

        let mut cells = vec![vec![Cell::BLANK; grid_cols]; grid_rows];
        for r in 0..grid_rows {
            for c in 0..grid_cols {
                if mask[r][c] != 0 {
                    cells[r][c] = Cell {
                        ch: BOX[mask[r][c] as usize],
                        fg: Some(t.border),
                        ..Cell::BLANK
                    };
                }
            }
        }

        // Pasada 2: panel actual resaltado, etiquetas y títulos.
        for (x0, y0, x1, y1, pane, label) in &rects {
            let (x0, y0, x1, y1) = (*x0, *y0, *x1, *y1);
            let focused = pane.is_focused;

            if focused {
                for y in (y0 + 1)..y1 {
                    for x in (x0 + 1)..x1 {
                        cells[y][x].bg = Some(t.focus_bg);
                    }
                }
                for x in x0..=x1 {
                    cells[y0][x].fg = Some(t.focus);
                    cells[y1][x].fg = Some(t.focus);
                }
                for y in y0..=y1 {
                    cells[y][x0].fg = Some(t.focus);
                    cells[y][x1].fg = Some(t.focus);
                }
            }

            let iw = x1 - x0 - 1;
            let ih = y1 - y0 - 1;
            let cy = (y0 + y1) / 2;

            // Etiqueta como "insignia" (solo si sigue coincidiendo con lo tecleado).
            if label.starts_with(&self.map_typed) {
                let tag = format!(" {} ", label.to_uppercase());
                let tag_len = tag.chars().count();
                let start = if iw >= tag_len {
                    x0 + 1 + (iw - tag_len) / 2
                } else {
                    (x0 + x1) / 2
                };
                let badge = if focused { t.focus } else { t.label };
                put(&mut cells, cy, start, &tag, Some(t.ink), Some(badge), true);
            }

            // Título debajo de la etiqueta, si cabe.
            if ih >= 2 && iw >= 4 {
                let raw = if pane.title.is_empty() {
                    pane.terminal_command.clone().unwrap_or_default()
                } else {
                    pane.title.clone()
                };
                let title: String = raw.chars().take(iw).collect();
                let title_len = title.chars().count();
                let (fg, bg) = if focused {
                    (t.focus, Some(t.focus_bg))
                } else {
                    (t.title, None)
                };
                put(
                    &mut cells,
                    cy + 1,
                    x0 + 1 + (iw - title_len) / 2,
                    &title,
                    Some(fg),
                    bg,
                    false,
                );
            }
        }

        // Salida con colores de 24 bits.
        let mut out = String::new();
        for r in 0..grid_rows {
            out.push_str(&format!("\x1b[{};1H", r + 1));
            let mut prev: Option<Cell> = None;
            for c in 0..grid_cols {
                let cell = cells[r][c];
                let changed = match prev {
                    Some(p) => !same_style(&p, &cell),
                    None => true,
                };
                if changed {
                    out.push_str(&sgr(&cell));
                }
                out.push(cell.ch);
                prev = Some(cell);
            }
            out.push_str("\x1b[0m");
        }

        // Barra inferior.
        let typed = if self.map_typed.is_empty() {
            String::new()
        } else {
            format!("  ›  {}", self.map_typed.to_uppercase())
        };
        let help: String = format!(
            "  {} paneles · elige una letra · esc cancela{}",
            rects.len(),
            typed
        )
        .chars()
        .take(cols.saturating_sub(7))
        .collect();
        let badge = Cell {
            ch: ' ',
            fg: Some(t.ink),
            bg: Some(t.label),
            bold: true,
        };
        let text = Cell {
            ch: ' ',
            fg: Some(t.title),
            ..Cell::BLANK
        };
        out.push_str(&format!(
            "\x1b[{};1H{} SALTO \x1b[0m{}{}\x1b[0m",
            rows,
            sgr(&badge),
            sgr(&text),
            help
        ));

        print!("{out}");
        std::io::stdout().flush().ok();
    }
}

type Rgb = (u8, u8, u8);

/// Colores del modo mapa. Todos son opcionales en la configuración del alias:
/// color_borde, color_titulo, color_letra, color_actual, color_fondo_actual
/// (en formato "#rrggbb").
struct Theme {
    border: Rgb,
    title: Rgb,
    label: Rgb,
    focus: Rgb,
    focus_bg: Rgb,
    ink: Rgb,
}

impl Theme {
    fn from_config(config: &BTreeMap<String, String>) -> Self {
        let get = |key: &str, default: Rgb| {
            config
                .get(key)
                .and_then(|v| parse_hex(v))
                .unwrap_or(default)
        };
        Theme {
            border: get("color_borde", (0x8b, 0x7b, 0xd8)),
            title: get("color_titulo", (0xc9, 0xc9, 0xe3)),
            label: get("color_letra", (0xff, 0x2a, 0x6d)),
            focus: get("color_actual", (0x05, 0xd9, 0xe8)),
            focus_bg: get("color_fondo_actual", (0x10, 0x2a, 0x3a)),
            ink: (0x0d, 0x02, 0x21),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Theme::from_config(&BTreeMap::new())
    }
}

fn parse_hex(s: &str) -> Option<Rgb> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let n = u32::from_str_radix(s, 16).ok()?;
    Some((
        ((n >> 16) & 0xff) as u8,
        ((n >> 8) & 0xff) as u8,
        (n & 0xff) as u8,
    ))
}

#[derive(Clone, Copy, PartialEq)]
struct Cell {
    ch: char,
    fg: Option<Rgb>,
    bg: Option<Rgb>,
    bold: bool,
}

impl Cell {
    const BLANK: Cell = Cell {
        ch: ' ',
        fg: None,
        bg: None,
        bold: false,
    };
}

fn same_style(a: &Cell, b: &Cell) -> bool {
    a.fg == b.fg && a.bg == b.bg && a.bold == b.bold
}

fn sgr(cell: &Cell) -> String {
    let mut s = String::from("\x1b[0");
    if cell.bold {
        s.push_str(";1");
    }
    if let Some((r, g, b)) = cell.fg {
        s.push_str(&format!(";38;2;{r};{g};{b}"));
    }
    if let Some((r, g, b)) = cell.bg {
        s.push_str(&format!(";48;2;{r};{g};{b}"));
    }
    s.push('m');
    s
}

fn put(
    cells: &mut [Vec<Cell>],
    row: usize,
    col: usize,
    s: &str,
    fg: Option<Rgb>,
    bg: Option<Rgb>,
    bold: bool,
) {
    if row >= cells.len() {
        return;
    }
    for (i, ch) in s.chars().enumerate() {
        let c = col + i;
        if c < cells[row].len() {
            cells[row][c] = Cell { ch, fg, bg, bold };
        }
    }
}

fn make_labels(n: usize, chars: &[char]) -> Vec<String> {
    if n <= chars.len() {
        return chars.iter().take(n).map(|c| c.to_string()).collect();
    }
    let mut labels = Vec::with_capacity(n);
    'outer: for a in chars {
        for b in chars {
            labels.push(format!("{a}{b}"));
            if labels.len() == n {
                break 'outer;
            }
        }
    }
    labels
}
