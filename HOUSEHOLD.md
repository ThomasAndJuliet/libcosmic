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
- **toaster:** progress, persistent toasts, and updating a toast in place.
- **popover:**
  - `Position::BelowAt`;
  - a modal keeps key presses and IME text from the content behind it.
- **text_editor:** reports its id only once when a context menu wraps it.
- **context_menu:** native popups are anchored at the window cursor in scrolled views.
- **segmented_button:**
  - the tab-drag candidate is forgotten on click;
  - a `handle_event` refactor;
  - `tab_drag_data`, `on_tab_drag_start` and `on_tab_hover`.
- **theme:** hidden `set_active`, for headless rendering in tests.
- **iced wgpu:** rasterised SVGs are un-premultiplied before upload.
- **iced scrollable:**
  - `Viewport::is_user_scroll` and `is_pinned_to_end`;
  - `initial_position` and `Viewport::position`. **Note:** an id set with `.id()` is no longer replaced by `set_id`, for every scrollable.
- **iced tiny_skia:** the clip mask is set for each raw text run, fill and stroke, and is only refilled when its bounds change.

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
