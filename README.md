# zellij-salto

Salta entre los paneles de la pestaña actual con una letra, al estilo de
[nvim-window](https://github.com/yorickpeterse/nvim-window).

Al activarlo, cada panel muestra su letra en el título del borde
(`[a] título`). Pulsas la letra y ese panel recibe el foco. Con más de 26
paneles usa dos letras. Esc, cualquier otra tecla o 8 segundos sin pulsar
nada cancelan y restauran los títulos.

## Compilar

```bash
cd ~/proyectos/zellij-arbol
rustup target add wasm32-wasip1
cargo build --release --target wasm32-wasip1
mkdir -p ~/.config/zellij/plugins
cp target/wasm32-wasip1/release/zellij_salto.wasm ~/.config/zellij/plugins/
```

## Configurar (`~/.config/zellij/config.kdl`)

```kdl
// Carga el plugin en segundo plano al iniciar cada sesión (no tiene panel).
load_plugins {
    "file:~/.config/zellij/plugins/zellij_salto.wasm"
}

keybinds {
    shared_except "locked" {
        bind "Alt w" {
            MessagePlugin "file:~/.config/zellij/plugins/zellij_salto.wasm" {
                name "salto"
            }
        }
    }
}
```

Si ya tienes un bloque `keybinds`, añade solo el `bind` dentro de él.

## Permisos

Pide `ReadApplicationState`, `ChangeApplicationState` e `InterceptInput`.
Si al iniciar la sesión no aparece el aviso para aceptarlos, ábrelo una vez a
mano y acepta con `y`:

```bash
zellij plugin -- file:~/.config/zellij/plugins/zellij_salto.wasm
```

Zellij guarda los permisos por URL del plugin.

## Ajustes en `src/main.rs`

- `CHARS`: letras usadas para las etiquetas.
- `TIMEOUT_SECS`: tiempo antes de cancelar solo.
- `RESTORE_WITH_EMPTY_NAME`: cómo se restauran los títulos. Con `false` cada
  panel queda con su título anterior como nombre fijo; con `true` se intenta
  volver al título dinámico.

## Notas

- Escrito contra `zellij-tile =0.45.1`. Si el compilador protesta por la firma
  de alguna función (por ejemplo `focus_terminal_pane`), ajústala a la de esa
  versión.
- Solo etiqueta paneles de terminal visibles de la pestaña activa. Los
  plugins (barras, etc.) y los paneles suprimidos se ignoran.
