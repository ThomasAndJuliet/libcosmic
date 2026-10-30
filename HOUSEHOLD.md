# The household fork of libcosmic

This branch, `household`, is pop-os/libcosmic with the changes the home-apps need. It is private. Nothing here goes upstream: pop-os accepts no AI-written code. So every change is carried and rebased for as long as we need it, and changes should stay small and additive.

Coordination between Tom's and Juliet's agents happens in `neo:/srv/household/agent-notes/libcosmic.md`; post there before and after changing this branch. The backlog is Juliet's list in ThomasAndJuliet/household-ui, `proposals/libcosmic.md`, together with `INVENTORY.md` in the staging area (`neo:/mnt/disk-array/Share/Projects/libcosmic-fork/`).

## Layout

| Path | What | Upstream |
|---|---|---|
| `/` | libcosmic | pop-os/libcosmic `e1f2cd2` |
| `iced/` | iced, as a subtree (upstream has a submodule) | pop-os/iced `a31e6c6`, the commit libcosmic pins |
| `cosmic-icons/` | icons, as a subtree | pop-os/cosmic-icons `343c007` |
| `winit/` | winit, as a subtree | pop-os/winit `9567503`, the commit iced pins |

iced still depends on pop-os/winit from git. The fork's winit copy is used through a `[patch]` of the whole winit family in each app, because `accesskit_winit` also depends on pop-os/winit directly. Making winit a path dependency of iced would bring in a second winit.

## Using it from an app

Keep the app's dependencies on pop-os URLs as they are, and redirect them in the app's root `Cargo.toml`. Use every URL spelling that appears in the app's `Cargo.lock`; Cargo treats `…/libcosmic` and `…/libcosmic.git` as different sources.

```toml
[patch."https://github.com/pop-os/libcosmic.git"]
libcosmic = { git = "FORK-URL", rev = "COMMIT" }
# and any of these the app depends on directly:
# cosmic-config, cosmic-theme, iced_accessibility, …

[patch."https://github.com/pop-os/winit.git"]
dpi = { git = "FORK-URL", rev = "COMMIT" }
winit = { git = "FORK-URL", rev = "COMMIT" }
winit-android = { git = "FORK-URL", rev = "COMMIT" }
winit-appkit = { git = "FORK-URL", rev = "COMMIT" }
winit-common = { git = "FORK-URL", rev = "COMMIT" }
winit-core = { git = "FORK-URL", rev = "COMMIT" }
winit-orbital = { git = "FORK-URL", rev = "COMMIT" }
winit-uikit = { git = "FORK-URL", rev = "COMMIT" }
winit-wayland = { git = "FORK-URL", rev = "COMMIT" }
winit-web = { git = "FORK-URL", rev = "COMMIT" }
winit-win32 = { git = "FORK-URL", rev = "COMMIT" }
winit-x11 = { git = "FORK-URL", rev = "COMMIT" }
```

Redirect the whole winit family, not only `winit-wayland`. The fork's winit crates refer to each other by path. So a lone `winit-wayland` from the fork would bring its own `winit-core`, which doesn't match the `winit-core` the rest of winit uses from pop-os, and winit fails to compile.

This replaces a vendored `libcosmic/` subtree and a vendored `patches/winit-wayland/`. A Flatpak build then needs its cargo sources regenerated.

## Changes

Each commit says where it came from, using an `Origin:` line or a household-ui proposal number.

- **winit:** drop pointer events for a released pointer; this fixes the "failed to get pointer data" panic.
- **iced_winit:** the SCTK thread is stopped and joined before winit's event loop disconnects the display; this fixes the segfault in libwayland when an app exits. A finished instance is never polled again, which fixes the "`async fn` resumed after completion" panic when an app quits while a window is being created. When the compositor goes away, the SCTK thread stops on the dead connection and logs it once, instead of dispatching it in a busy loop until the join times out.
- **toaster:** progress, persistent toasts, and updating a toast in place.
- **popover:**
  - `Position::BelowAt`;
  - a modal keeps key presses and IME text from the content behind it.
- **text_editor:** reports its id only once when a context menu wraps it;
  redraws selected text over its fill, using the accent's on colour by
  default, with an additive `selected_text_color` callback for an app's
  fill-colour rule (as one-line text inputs already do).
- **selection (bug fix):** text inputs and text editors draw the selection
  fill in each selection rectangle's own layer, with the selected text
  redrawn there, so the fill covers the first text pass and its glyphs no
  longer fringe the redrawn ones (a layer draws its quads before its text).
  iced's editor and libcosmic's `TextEditor` have an additive
  `selection_fill` callback, so an app can supply its own opaque fill.
  Colours are upstream's: the style's selection fill, and the theme's on
  colour for the text. The household colour rule for selections lives in
  the apps, through `household-tones` (Juliet, 25 Oct: "keep libcosmic close
  to the original").
- **text_input (bug fix):** a key held with Alt or Super is not typed and is
  not captured, so it reaches the app's shortcuts. Ctrl keys are refused as
  before, and stay captured. Shift, AltGr (which xkb reports as level 3, not
  Alt), dead keys and input methods still type; Option still types on macOS.
- **context_menu:** native popups are anchored at the window cursor in scrolled views;
  menus inherit the window currently building them, as text-field menus do,
  instead of targeting the reserved placeholder window; the Esc that closes a
  menu is captured, so an app's Esc shortcut does not run too.
- **segmented_button:**
  - the tab-drag candidate is forgotten on click;
  - a `handle_event` refactor;
  - `tab_drag_data`, `on_tab_drag_start` and `on_tab_hover`.
  - an optional title colour per item, `Model::text_color` and
    `Model::text_color_set`: the title is drawn in it in every state, in place
    of the state's text colour; the icon, the close button and the drop marker
    keep the state's colour (household-ui proposal #39).
- **menus:** `menu::Entry::selected(bool)` draws an entry as one choice of
  several, with no check column: the chosen one is filled with the menu's
  selected colour (`selected_state_color`), its label and icon in the accent
  text colour, semibold, as a dropdown draws its chosen option; an unchosen
  one is an ordinary entry. It goes through `menu::items`, so the expanded
  and the collapsed `responsive_menu_bar` both draw it. `CheckBox` and
  `Entry::checked` are unchanged (T-076, Juliet 14 Oct).
  `Entry::mark(handle)` draws a 16 px icon in the check column in place of
  the tick, in the row's icon colour (the accent text colour on a selected
  row): a radio mark for one choice of several (Juliet, 19 Oct: "we also
  need a radio box like icon").
- **theme:** hidden `set_active`, for headless rendering in tests.
- **cosmic-theme:** the background layer's hover and selected take its own
  overlay again, not the one the primary block leaves behind (white in light
  mode since upstream 25bf8f6), so light menus show a hover: #dcdcdc on
  #f5f5f5, as before that commit; dark is unchanged. The Qt light palette's
  HighlightedText follows (cosmic-epoch#3668, Juliet: "34 yes"). The
  transparent background's hover, selected and pressed take the same
  overlays, not the ones the secondary block leaves behind.
- **iced wgpu:** rasterised SVGs are un-premultiplied before upload.
- **iced scrollable:**
  - `Viewport::is_user_scroll` and `is_pinned_to_end`;
  - `initial_position` and `Viewport::position`. **Note:** an id set with `.id()` is no longer replaced by `set_id`, for every scrollable.
- **iced tiny_skia:** the clip mask is set for each raw text run, fill and stroke, and is only refilled when its bounds change.

### Round 1 (16 October 2026)

Each commit carries the number of the household-ui proposal it answers, or an `Inventory:` line.

- **SVG:** a tint's alpha is applied in both renderers.
- **settings:**
  - flex rows centre their items vertically (`align_items`, not the ignored `justify_items`) (#8);
  - opt-in `settings::control_lines(row, n)`, plus `Item::control_lines` and `Item::flex_control_lines`, centre the control on the first `n` lines of text (#8).
- **toggler:** opt-in `theme::Toggler` class, with `accent(colour)` and `custom(style)` variants. The default look is unchanged (#10).
- **button:**
  - `align_x` and `align_y` place a button's content;
  - text and icon buttons centre their content, and honour a `Fill` width (#7).
- **menus:**
  - the menu bar diffs its roots' own trees, which fixes a panic when the roots change kind (#24);
  - the press that closes a menu is swallowed, for both in-window and native popups (#17).
  - an open menu whose items change under it takes its items' slots again from the tree it shows: a shorter menu no longer indexes past its end (a panic in Lollypop, 25 Oct), and a longer one, or one whose items changed kind, is no longer drawn in the old slots; a submenu whose item is gone closes.
  - an in-window menu that fits the window but not beside its anchor shifts just enough to lie wholly inside it; one that fits there, and one taller than the window, open as before.
- **button:** overlays of a button's content (a tooltip on a row's title, the heart's tooltip in a row) open at the content, not a button's offset away: the button passes the translation on as iced's containers do.
- **cosmic-theme:**
  - a partially saved theme is filled from its own mode's defaults;
  - a never-saved light theme gives the built-in light theme;
  - `with_accent` rebuilds the theme it is called on, without reading from disk.
- **app:**
  - the core starts with the saved desktop theme;
  - system theme hooks run after the theme has changed, and their tasks are no longer dropped on Wayland;
  - an optional single-instance bus name, `Settings::single_instance_name` (#27).
- **core:**
  - public scale, size and exit-on-main-window-closed setters (#30);
  - `Core::scale_factor_of(window::Id)` (#35).
- **iced_winit:** SCTK input-method events are forwarded according to the surface, not the `single-instance` feature (#29).

### From cosmic-term, 17 October

Changes Tom's cosmic-term sessions made to its vendored copy. Each commit has an `Origin: cosmic-term <sha>` line.

- **iced:**
  - graphics and window failures are reported, not panics: `Compositor` methods return a `Result`, `window::Event::RenderingFailed` closes only the affected window, and `window::try_open` and `window::try_screenshot` report failures. **Note:** this changes the `Compositor` trait and the channels of `window::Action::Open` and `Screenshot`;
  - iced_winit survives a cancelled runtime and missing Wayland handles during shutdown;
  - `operation::in_window`, and `focus(id)` changes focus only in the target's window.
- **app:**
  - keyboard navigation stays in the focused window;
  - `Application::owns_window`: an instance drains only its own deferred text popups;
  - `Cosmic::close_owned_surfaces` closes an instance's surfaces, children first.
- **menus:**
  - `Menu::menu_roots` is a `Vec`; every menu owns its roots;
  - a menu bar closes native popups built for an older model when its roots change shape.
- **popover:** `Position::BelowAtWindow`.
- **text_context_menu:** a second right-click replaces the popup, and a replaced surface no longer resets the widget.

### Widget state reconciliation, 27 October

Named children now move their retained trees to their new positions during
reconciliation. Inserting a child before a named sibling keeps both children;
removing an earlier sibling preserves the named child's state. Named state
restoration checks the saved widget tag, and a duplicate name cannot reuse
an empty placeholder left by extraction. Widget state downcast assertions
remain intact.

Run the focused regressions from `iced/` with
`cargo test --locked -p iced_core --lib widget::tree -- --test-threads=1`.

### Small bug fixes, 29 October

Juliet, 25 Oct: "the fork carries small bug fixes". Each commit has an `Origin:` line.

- **iced list:** a rebuilt `List` builds its visible rows in `update` and
  `operate` when it has none yet, as its redraw does, so a press or a Tab
  focus step between a rebuild and the next frame reaches the rows instead of
  falling through (Specula; its `list_rows::catch_presses` works around it
  until its pin has this). Run from `iced/` with
  `cargo test --locked -p iced_widget --lib list::tests`.
- **segmented_button:** the focus ring is drawn in the theme's accent text
  colour, not in the style's `active.text_color`, which a style may mean only
  for its chosen part's own fill (a light title on a filled part left the ring
  near-invisible on the unchosen parts; Marginalium). The built-in styles give
  `active.text_color` that same colour, so they look as before.
- **segmented_button:** a hovered or pressed part lays the style's wash over
  its own background, as a button lays its overlay, instead of replacing it,
  so a chosen part with a fill of its own keeps it under the hover (found in
  Marginalium). Parts with no background of their own (every unchosen part in
  the built-in styles) look as before; in the built-in styles the chosen
  part, whose own fill is the same grey as the wash, is a little darker when
  hovered or pressed (Control: 28 % / 24 % grey instead of 20 % / 15 %; tabs:
  44 % / 40 % instead of 30 % / 25 %).
- **segmented_button:** a hovered or pressed chosen part keeps the active
  state's text and icon colour, meant for its own fill, instead of taking the
  hover's (accent text on a muted chosen fill under the wash was low contrast;
  Deductum, Marginalium). An unchosen part still takes the hover's colour. The
  built-in styles' hover text colour is their active one, so they look as
  before.
- **segmented_button:** a horizontal control built with dividers (as
  `segmented_control::horizontal` builds it) draws them in any style, not only
  in `Control`, so a `Custom` style that starts from `Control`'s appearance
  keeps the 1 px dividers between parts (Marginalium lost the one between two
  unchosen parts). Only `segmented_control` turns dividers on, always with
  `Control`, so the built-in looks are unchanged. The chosen part's tick is
  still `Control`'s alone.

## CI

`.github/workflows/household.yml` runs on neo's `neo-ubuntu` runner for pushes to `household` and `fork/**` by Tom or Juliet; nothing runs on GitHub-hosted runners. Upstream's workflows are removed. It runs:

- libcosmic's library tests;
- a check with the apps' feature set;
- the popup lifecycle and menu-model tests that used to run in cosmic-term's CI;
- `tests/exit_teardown.rs`: a minimal app quits under a headless weston, with and without a popup, and must exit 0 with no SCTK thread left; when its compositor is killed under it, it must end within 10 s, with the SCTK thread joined after reporting the dead connection in at most 2 lines (needs `weston`; fails without it);
- cosmic-theme's tests;
- iced's widget state reconciliation, widget tests (the list's rows after a rebuild), renderer failure tests and SVG tests;
- a check of winit's Wayland backend.

Push a `fork/<topic>` branch to get a run before merging it into `household`.

## Rebasing onto upstream

1. Fetch upstream libcosmic, and rebase `household` onto the new upstream commit, dropping the subtree commits.
2. Re-add each subtree at the commit the new upstream pins:
   - iced: `git ls-tree <upstream> iced`;
   - winit: iced's `Cargo.toml`;
   - icons.

   Alternatively, use `git subtree pull --prefix=iced --squash`.
3. Re-apply the iced and winit commits with `git format-patch` and `git am -3 --directory=iced` (or `winit`).
4. Drop any change that upstream has since made itself, and say so in the shared note.
5. Build at least cosmic-term and deductum against the result before moving any app's pin.
