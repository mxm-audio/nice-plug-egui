//! MXM PATCH (*Host keys*): the keys a host hands an egui editor instead of letting its window have
//! them, given to egui as the window's own would be.
//!
//! An editor asks for them with [`EguiNiceSettings::with_host_keys`](crate::EguiNiceSettings),
//! naming which keys it wants. Its window then registers a receiver with nice-plug's
//! [`host_keys`](nice_plug::host_keys) for as long as it is open; a key the host offers that the
//! editor wants is queued, taken (so the host doesn't use it too), and put into egui's input at
//! the start of the editor's next frame, as an `egui::Event::Key` with its physical key.
//!
//! **A key a host holds back has no release from it**: Ableton Live 11 offers letters through
//! `onKeyDown` and sends no `onKeyUp` (2026-10-16). So on Windows a key taken this way is released
//! when the keyboard says it is up, read each frame while any is held; a host's own release, where
//! it sends one, comes first. Elsewhere only a host's release ends it.

use egui::Key;
use nice_plug::host_keys::HostKey;
use nice_plug_core::editor::{Modifiers, VirtualKeyCode};
use parking_lot::Mutex;
use std::sync::Arc;

/// Which keys an editor wants from its host: called with the window's egui context, on the host's
/// GUI thread, between frames.
pub type WantsKey = Arc<dyn Fn(&egui::Context, Key, egui::Modifiers) -> bool + Send + Sync>;

/// How often a held key's release is looked for.
const RELEASE_POLL: std::time::Duration = std::time::Duration::from_millis(30);

/// The keys taken from the host and not yet given to egui, and those still held.
#[derive(Default)]
pub(crate) struct Taken {
    events: Vec<egui::Event>,
    held: Vec<Key>,
}

/// The receiver one window registers: what [`nice_plug::host_keys::offer`] calls.
pub(crate) fn receiver(
    wants: WantsKey,
    egui_ctx: Arc<Mutex<Option<egui::Context>>>,
    taken: Arc<Mutex<Taken>>,
    repaint: crate::RepaintNotifier,
) -> nice_plug::host_keys::Receiver {
    Arc::new(move |key, down, modifiers| {
        let Some(key) = egui_key(key) else {
            return false;
        };
        let modifiers = egui_modifiers(modifiers);
        let Some(ctx) = egui_ctx.lock().clone() else {
            return false;
        };
        if !wants(&ctx, key, modifiers) {
            return false;
        }
        take(&mut taken.lock(), key, down, modifiers);
        repaint.request_repaint();
        true
    })
}

/// Queues `key` going down or up, and keeps track of it held.
fn take(taken: &mut Taken, key: Key, down: bool, modifiers: egui::Modifiers) {
    let held = taken.held.contains(&key);
    if !down && !held {
        return;
    }
    if down && !held {
        taken.held.push(key);
    } else if !down {
        taken.held.retain(|k| *k != key);
    }
    taken
        .events
        .push(key_event(key, down, down && held, modifiers));
}

/// At the start of a frame: the keys taken since the last one, into egui's input, with a release
/// for each held key the keyboard now says is up. Asks for another frame while any is held, so its
/// release is found.
pub(crate) fn give(ctx: &egui::Context, taken: &Mutex<Taken>) {
    let events = {
        let mut taken = taken.lock();
        for key in released(&taken.held) {
            taken.held.retain(|k| *k != key);
            let modifiers = ctx.input(|i| i.modifiers);
            taken.events.push(key_event(key, false, false, modifiers));
        }
        if !taken.held.is_empty() {
            ctx.request_repaint_after(RELEASE_POLL);
        }
        std::mem::take(&mut taken.events)
    };
    if !events.is_empty() {
        ctx.input_mut(|input| {
            input.events.splice(0..0, events);
        });
    }
}

fn key_event(key: Key, pressed: bool, repeat: bool, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: Some(key),
        pressed,
        repeat,
        modifiers,
    }
}

/// The egui key a host's key is: a letter, a digit or a mark by its character, a virtual key by
/// name. `None` for one egui has no key for (a modifier alone, a media key).
pub(crate) fn egui_key(key: HostKey) -> Option<Key> {
    match key {
        HostKey::Character(c) => Key::from_name(c.encode_utf8(&mut [0; 4])),
        HostKey::Virtual(v) => Some(match v {
            VirtualKeyCode::ArrowLeft => Key::ArrowLeft,
            VirtualKeyCode::ArrowRight => Key::ArrowRight,
            VirtualKeyCode::ArrowUp => Key::ArrowUp,
            VirtualKeyCode::ArrowDown => Key::ArrowDown,
            VirtualKeyCode::Escape => Key::Escape,
            VirtualKeyCode::Return | VirtualKeyCode::NumpadEnter => Key::Enter,
            VirtualKeyCode::Tab => Key::Tab,
            VirtualKeyCode::Space => Key::Space,
            VirtualKeyCode::Backspace => Key::Backspace,
            VirtualKeyCode::Delete => Key::Delete,
            VirtualKeyCode::Insert => Key::Insert,
            VirtualKeyCode::Home => Key::Home,
            VirtualKeyCode::End => Key::End,
            VirtualKeyCode::PageUp => Key::PageUp,
            VirtualKeyCode::PageDown => Key::PageDown,
            VirtualKeyCode::F1 => Key::F1,
            VirtualKeyCode::F2 => Key::F2,
            VirtualKeyCode::F3 => Key::F3,
            VirtualKeyCode::F4 => Key::F4,
            VirtualKeyCode::F5 => Key::F5,
            VirtualKeyCode::F6 => Key::F6,
            VirtualKeyCode::F7 => Key::F7,
            VirtualKeyCode::F8 => Key::F8,
            VirtualKeyCode::F9 => Key::F9,
            VirtualKeyCode::F10 => Key::F10,
            VirtualKeyCode::F11 => Key::F11,
            VirtualKeyCode::F12 => Key::F12,
            _ => return None,
        }),
    }
}

fn egui_modifiers(modifiers: Modifiers) -> egui::Modifiers {
    let command = modifiers.contains(Modifiers::COMMAND);
    egui::Modifiers {
        alt: modifiers.contains(Modifiers::ALT),
        ctrl: command || modifiers.contains(Modifiers::CONTROL),
        shift: modifiers.contains(Modifiers::SHIFT),
        mac_cmd: cfg!(target_os = "macos") && command,
        command,
    }
}

/// The held keys the keyboard says are up now: on Windows, `GetAsyncKeyState`; elsewhere none, and
/// only a host's own release ends a key.
#[cfg(windows)]
fn released(held: &[Key]) -> Vec<Key> {
    held.iter()
        .copied()
        .filter(|key| virtual_key(*key).is_some_and(|vk| !windows::down(vk)))
        .collect()
}

#[cfg(not(windows))]
fn released(_held: &[Key]) -> Vec<Key> {
    Vec::new()
}

/// The Windows virtual-key code of an egui key: a letter's or a digit's is its uppercase ASCII
/// code, a mark's the one the keyboard layout gives it.
#[cfg(windows)]
fn virtual_key(key: Key) -> Option<i32> {
    let name = key.name();
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next())
        && c.is_ascii_alphanumeric()
    {
        return Some(c.to_ascii_uppercase() as i32);
    }
    Some(match key {
        Key::ArrowLeft => 0x25,
        Key::ArrowUp => 0x26,
        Key::ArrowRight => 0x27,
        Key::ArrowDown => 0x28,
        Key::Escape => 0x1B,
        Key::Enter => 0x0D,
        Key::Tab => 0x09,
        Key::Space => 0x20,
        Key::Backspace => 0x08,
        Key::Delete => 0x2E,
        Key::Insert => 0x2D,
        Key::Home => 0x24,
        Key::End => 0x23,
        Key::PageUp => 0x21,
        Key::PageDown => 0x22,
        Key::F1 => 0x70,
        Key::F2 => 0x71,
        Key::F3 => 0x72,
        Key::F4 => 0x73,
        Key::F5 => 0x74,
        Key::F6 => 0x75,
        Key::F7 => 0x76,
        Key::F8 => 0x77,
        Key::F9 => 0x78,
        Key::F10 => 0x79,
        Key::F11 => 0x7A,
        Key::F12 => 0x7B,
        other => return windows::virtual_key_of(other.symbol_or_name().chars().next()?),
    })
}

#[cfg(windows)]
mod windows {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetAsyncKeyState(key: i32) -> i16;
        fn VkKeyScanW(character: u16) -> i16;
    }

    /// Whether the key is down now, whichever window has the keyboard.
    pub(super) fn down(virtual_key: i32) -> bool {
        // SAFETY: a plain query of the keyboard's state.
        unsafe { GetAsyncKeyState(virtual_key) as u16 & 0x8000 != 0 }
    }

    /// The virtual-key code the keyboard layout gives a character, `None` if it has none.
    pub(super) fn virtual_key_of(character: char) -> Option<i32> {
        let mut units = [0; 2];
        let [unit] = character.encode_utf16(&mut units) else {
            return None;
        };
        // SAFETY: a plain query of the keyboard layout.
        let scan = unsafe { VkKeyScanW(*unit) };
        (scan != -1).then_some(i32::from(scan as u16 & 0xFF))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_key_is_the_egui_key_of_the_same_name() {
        assert_eq!(egui_key(HostKey::Character('S')), Some(Key::S));
        assert_eq!(egui_key(HostKey::Character('1')), Some(Key::Num1));
        assert_eq!(egui_key(HostKey::Character(';')), Some(Key::Semicolon));
        assert_eq!(
            egui_key(HostKey::Virtual(VirtualKeyCode::ArrowUp)),
            Some(Key::ArrowUp)
        );
        assert_eq!(egui_key(HostKey::Virtual(VirtualKeyCode::Shift)), None);
    }

    #[test]
    fn a_taken_key_goes_down_once_repeats_while_held_and_comes_up_once() {
        let mut taken = Taken::default();
        let none = egui::Modifiers::NONE;
        take(&mut taken, Key::S, true, none);
        take(&mut taken, Key::S, true, none);
        take(&mut taken, Key::S, false, none);
        take(&mut taken, Key::S, false, none);
        let seen: Vec<(bool, bool)> = taken
            .events
            .iter()
            .map(|event| match event {
                egui::Event::Key {
                    pressed, repeat, ..
                } => (*pressed, *repeat),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(seen, [(true, false), (true, true), (false, false)]);
        assert!(taken.held.is_empty());
    }

    #[test]
    fn taken_keys_reach_egui_at_the_start_of_the_next_frame_in_order() {
        let ctx = egui::Context::default();
        let taken = Mutex::new(Taken::default());
        take(&mut taken.lock(), Key::S, true, egui::Modifiers::NONE);
        take(&mut taken.lock(), Key::ArrowUp, true, egui::Modifiers::NONE);
        ctx.begin_pass(egui::RawInput::default());
        give(&ctx, &taken);
        let seen: Vec<Key> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key {
                        key, pressed: true, ..
                    } => Some(*key),
                    _ => None,
                })
                .collect()
        });
        // The frame's textures are no test's to upload, and egui insists they be handled.
        ctx.end_pass().textures_delta.clear();
        assert_eq!(seen, [Key::S, Key::ArrowUp]);
        assert!(taken.lock().events.is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn a_letter_and_an_arrow_have_their_windows_key_codes() {
        assert_eq!(virtual_key(Key::S), Some(0x53));
        assert_eq!(virtual_key(Key::Num1), Some(0x31));
        assert_eq!(virtual_key(Key::ArrowUp), Some(0x26));
    }
}
