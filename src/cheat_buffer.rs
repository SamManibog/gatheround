use std::{collections::VecDeque, ops::Sub};

use crate::platform::IKeyboardReader;

/// a simple text buffer for running lua commands
#[derive(Debug)]
pub struct CheatBuffer {
    /// command history
    history: VecDeque<String>,

    /// index in command history
    history_index: usize,

    /// command being entered now
    command: String,

    /// byte index in the command
    /// invariant: index always lies on a valid utf8 char boundary
    index: usize,

    /// the current frame used to calculate cursor blinking effects
    secs_since_input: f32,

    /// frame-based cooldown for holding up key
    up_cooldown: f32,

    /// frame-based cooldown for holding down key
    down_cooldown: f32,

    /// frame-based cooldown for holding left key
    left_cooldown: f32,

    /// frame-based cooldown for holding right key
    right_cooldown: f32,

    /// frame-based cooldown for holding delete key
    del_cooldown: f32,

    /// frame-based cooldown for holding backspace key
    bs_cooldown: f32,

    /// check if enter key was released since last press
    is_enter_released: bool,
}

impl CheatBuffer {
    pub const MAX_HISTORY: usize = 250;

    const SPAM_COOLDOWN: f32 = 1.0 / 12.0;

    pub fn new() -> Self {
        Self {
            history: VecDeque::with_capacity(Self::MAX_HISTORY),
            history_index: 0,
            command: String::new(),
            index: 0,
            secs_since_input: 0.0,
            up_cooldown: 0.0,
            down_cooldown: 0.0,
            left_cooldown: 0.0,
            right_cooldown: 0.0,
            del_cooldown: 0.0,
            bs_cooldown: 0.0,
            is_enter_released: true,
        }
    }

    /// decrements all cooldowns
    fn decrement_cooldowns(&mut self) {
        self.up_cooldown = self.up_cooldown.sub(1.0).max(0.0);
        self.down_cooldown = self.down_cooldown.sub(1.0).max(0.0);
        self.left_cooldown = self.left_cooldown.sub(1.0).max(0.0);
        self.right_cooldown = self.right_cooldown.sub(1.0).max(0.0);
        self.del_cooldown = self.del_cooldown.sub(1.0).max(0.0);
        self.bs_cooldown = self.bs_cooldown.sub(1.0).max(0.0);
    }

    /// increments the cursor
    fn increment_cursor(&mut self) {
        if let Some(ch) = self.command[self.index..].chars().next() {
            self.index += ch.len_utf8();
        }
    }

    /// decrements the cursor
    fn decrement_cursor(&mut self) {
        self.index = self.index.saturating_sub(1);

        while self.index > 0 && !self.command.is_char_boundary(self.index) {
            self.index -= 1;
        }
    }

    /// gets the currently entered command
    pub fn command(&self) -> &str {
        &self.command
    }

    /// gets the character at the cursor if it exists
    pub fn cursor_str(&self) -> Option<&str> {
        if self.index == self.command.len() {
            return None;
        }

        let mut chars = self.command[self.index..].chars();

        chars.next().map(|c| {
            let end_idx = self.index + c.len_utf8();
            &self.command[self.index..end_idx]
        })
    }

    /// gets the index of the cursor within the string
    /// note that this is the byte index, not the character index
    pub fn cursor_index(&self) -> usize {
        self.index
    }

    /// gets the number of frames since an input was received
    pub fn secs_since_input(&self) -> f32 {
        self.secs_since_input
    }

    /// updates the buffer with keyboard inputs
    pub fn update<I: IKeyboardReader>(
        &mut self,
        inputs: &mut I,
        delta_time: f32,
    ) -> Option<String> {
        self.secs_since_input += delta_time;
        self.decrement_cooldowns();

        // history traversal
        if inputs.is_up_key_pressed() {
            if self.up_cooldown <= 0.0 {
                self.secs_since_input = 0.0;
                self.up_cooldown = Self::SPAM_COOLDOWN;

                self.history_index = self.history_index.saturating_sub(1);
                if let Some(hist_command) = self.history.get(self.history_index) {
                    self.command = hist_command.clone();
                } else {
                    self.command = String::new();
                }
                self.index = self.command.len();
            }
        } else {
            self.up_cooldown = 0.0;
        }
        if inputs.is_down_key_pressed() {
            if self.down_cooldown <= 0.0 {
                self.secs_since_input = 0.0;
                self.down_cooldown = Self::SPAM_COOLDOWN;

                if self.history_index < self.history.len().saturating_sub(1) {
                    self.history_index += 1;
                    self.command = self.history[self.history_index].clone();
                }
                self.index = self.command.len();
            }
        } else {
            self.down_cooldown = 0.0;
        }

        // handle normal character entry
        while let Some(ch) = inputs.poll_char_press() {
            self.secs_since_input = 0.0;

            self.command.insert(self.index, ch);
            self.index += 1;
        }

        // handle movement
        if inputs.is_left_key_pressed() {
            if self.left_cooldown <= 0.0 {
                self.secs_since_input = 0.0;
                self.left_cooldown = Self::SPAM_COOLDOWN;

                self.decrement_cursor();
            }
        } else {
            self.left_cooldown = 0.0;
        }
        if inputs.is_right_key_pressed() {
            if self.right_cooldown <= 0.0 {
                self.secs_since_input = 0.0;
                self.right_cooldown = Self::SPAM_COOLDOWN;

                self.increment_cursor();
            }
        } else {
            self.right_cooldown = 0.0;
        }

        // handle deletion
        if inputs.is_backspace_key_pressed() {
            if self.bs_cooldown <= 0.0 {
                self.secs_since_input = 0.0;
                self.bs_cooldown = Self::SPAM_COOLDOWN;

                self.decrement_cursor();
                if let Some((idx, _)) = self.command.char_indices().nth(self.index) {
                    self.command.remove(idx);
                }
            }
        } else {
            self.bs_cooldown = 0.0;
        }
        if inputs.is_delete_key_pressed() {
            if self.del_cooldown <= 0.0 {
                self.secs_since_input = 0.0;
                self.del_cooldown = Self::SPAM_COOLDOWN;

                if let Some((idx, _)) = self.command.char_indices().nth(self.index) {
                    self.command.remove(idx);
                }
            }
        } else {
            self.del_cooldown = 0.0;
        }

        // handle the enter key
        if inputs.is_enter_key_pressed() {
            if self.is_enter_released {
                if self.command.is_empty() {
                    return None;
                }

                self.secs_since_input = 0.0;
                // adjust history
                if self.history.len() >= Self::MAX_HISTORY {
                    self.history.pop_front();
                }
                self.history.push_back(self.command.clone());
                self.history_index = self.history.len();

                self.index = 0;
                let mut output = String::new();
                std::mem::swap(&mut output, &mut self.command);
                Some(output)
            } else {
                None
            }
        } else {
            self.is_enter_released = true;
            None
        }
    }
}
