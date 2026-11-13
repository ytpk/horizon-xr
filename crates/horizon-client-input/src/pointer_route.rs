//! Which of two menu lasers drives the mouse. With both hands pointing
//! (`horizonBothHandPointers`) each controller has its own laser on the panel; the game has one
//! cursor and one left and right button, so the hand that clicks owns them until it lets go.

/// A hand's laser: where it meets the panel (0..1 from its top left) and its left and right
/// buttons (the trigger, the face button).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Laser {
    pub at: [f32; 2],
    pub buttons: [bool; 2],
}

/// What the mouse does this frame. `hand` indexes the lasers (0 main, 1 off).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Routed {
    /// The laser the cursor sits at, none when neither meets the panel.
    pub hand: Option<usize>,
    /// The mouse's left and right buttons.
    pub buttons: [bool; 2],
    /// The hand holding a button, to carry into the next frame.
    pub holder: Option<usize>,
}

/// `holder` is the hand that held a button last frame. A hand that presses while the other is
/// free takes the mouse (the main hand when both press in one frame) and keeps it, its drag
/// included, until its buttons are up or its laser leaves the panel. With no button down the
/// cursor follows the main hand, or the off hand when the main one misses the panel.
pub fn route(lasers: [Option<Laser>; 2], holder: Option<usize>) -> Routed {
    let down = |hand: usize| lasers[hand].filter(|l| l.buttons.iter().any(|b| *b));
    let holder = match holder {
        Some(h) if down(h).is_some() => Some(h),
        _ => (0..2).find(|h| down(*h).is_some()),
    };
    match holder {
        Some(h) => Routed {
            hand: Some(h),
            buttons: lasers[h].map_or([false; 2], |l| l.buttons),
            holder,
        },
        None => Routed {
            hand: (0..2).find(|h| lasers[*h].is_some()),
            buttons: [false; 2],
            holder: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn laser(x: f32, left: bool, right: bool) -> Option<Laser> {
        Some(Laser {
            at: [x, 0.5],
            buttons: [left, right],
        })
    }

    #[test]
    fn the_cursor_follows_the_main_hand_and_the_off_hand_when_the_main_one_misses() {
        let both = route([laser(0.2, false, false), laser(0.8, false, false)], None);
        assert_eq!(both.hand, Some(0));
        let off_only = route([None, laser(0.8, false, false)], None);
        assert_eq!(off_only.hand, Some(1));
        assert_eq!(route([None, None], None), Routed::default());
    }

    #[test]
    fn the_off_hands_trigger_clicks_at_its_own_point() {
        let routed = route([laser(0.2, false, false), laser(0.8, true, false)], None);
        assert_eq!(routed.hand, Some(1));
        assert_eq!(routed.buttons, [true, false]);
        assert_eq!(routed.holder, Some(1));
    }

    #[test]
    fn the_hand_that_holds_keeps_the_mouse_while_the_other_presses() {
        // The off hand drags; the main hand's trigger does not steal the button or the cursor.
        let routed = route([laser(0.2, true, false), laser(0.8, true, false)], Some(1));
        assert_eq!((routed.hand, routed.buttons), (Some(1), [true, false]));
        // Let go, and the main hand's press (still down) takes it on the next frame.
        let routed = route([laser(0.2, true, false), laser(0.8, false, false)], Some(1));
        assert_eq!((routed.hand, routed.holder), (Some(0), Some(0)));
    }

    #[test]
    fn a_hold_ends_when_the_laser_leaves_the_panel() {
        let routed = route([laser(0.2, false, false), None], Some(1));
        assert_eq!(
            (routed.hand, routed.buttons, routed.holder),
            (Some(0), [false; 2], None)
        );
    }

    #[test]
    fn the_main_hand_wins_a_press_in_the_same_frame() {
        let routed = route([laser(0.2, true, false), laser(0.8, true, false)], None);
        assert_eq!(routed.holder, Some(0));
    }

    #[test]
    fn the_face_button_is_the_right_click_of_the_hand_that_holds() {
        let routed = route([laser(0.2, false, false), laser(0.8, false, true)], None);
        assert_eq!((routed.hand, routed.buttons), (Some(1), [false, true]));
    }
}
