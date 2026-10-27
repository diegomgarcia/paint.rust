use paint_app::{AppSession, Command};
use paint_core::{Bgra, Point, Rect, Size, Surface};
use paint_document::{Document, Layer, LayerId};
use proptest::prelude::*;

fn session(size: Size, color: Bgra) -> AppSession {
    let surface = Surface::filled(size, color).expect("property surface");
    let layer = Layer::new(LayerId::new(1), "Background", surface).with_background(true);
    AppSession::new(Document::from_layers(size, vec![layer]).expect("property document"))
}

fn color() -> impl Strategy<Value = Bgra> {
    (any::<u8>(), any::<u8>(), any::<u8>(), any::<u8>())
        .prop_map(|(b, g, r, a)| Bgra::new(b, g, r, a))
}

#[derive(Clone, Copy, Debug)]
enum Operation {
    Fill(Bgra),
    Invert,
    Undo,
    Redo,
}

fn operation() -> impl Strategy<Value = Operation> {
    prop_oneof![
        4 => color().prop_map(Operation::Fill),
        2 => Just(Operation::Invert),
        3 => Just(Operation::Undo),
        3 => Just(Operation::Redo),
    ]
}

#[derive(Debug)]
struct HistoryModel {
    baseline: Bgra,
    current: Bgra,
    undo: Vec<Bgra>,
    redo: Vec<Bgra>,
}

impl HistoryModel {
    fn new(initial: Bgra) -> Self {
        Self {
            baseline: initial,
            current: initial,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    fn apply(&mut self, operation: Operation) {
        match operation {
            Operation::Fill(color) => self.change(color),
            Operation::Invert => self.change(self.current.inverted_rgb()),
            Operation::Undo => {
                if let Some(previous) = self.undo.pop() {
                    self.redo.push(self.current);
                    self.current = previous;
                }
            }
            Operation::Redo => {
                if let Some(next) = self.redo.pop() {
                    self.undo.push(self.current);
                    self.current = next;
                }
            }
        }
    }

    fn change(&mut self, next: Bgra) {
        if next != self.current {
            self.undo.push(self.current);
            self.current = next;
            self.redo.clear();
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn arbitrary_fill_undo_redo_sequences_match_an_independent_model(
        initial in color(),
        operations in prop::collection::vec(operation(), 1..48),
    ) {
        let size = Size::new(1, 1).unwrap();
        let mut session = session(size, initial);
        let mut model = HistoryModel::new(initial);

        for operation in operations {
            let command = match operation {
                Operation::Fill(color) => Command::Fill(color),
                Operation::Invert => Command::Invert,
                Operation::Undo => Command::Undo,
                Operation::Redo => Command::Redo,
            };
            session.execute(command).unwrap();
            model.apply(operation);

            let actual = *session.document().layers()[0]
                .surface()
                .pixel(Point::new(0, 0))
                .unwrap();
            prop_assert_eq!(actual, model.current);
            prop_assert_eq!(session.dirty(), model.current != model.baseline);
            prop_assert_eq!(session.can_undo(), !model.undo.is_empty());
            prop_assert_eq!(session.can_redo(), !model.redo.is_empty());
        }
    }

    #[test]
    fn signed_selection_fill_is_clipped_and_every_history_cycle_restores_pixels(
        width in 1_u32..7,
        height in 1_u32..7,
        left in -8_i64..8,
        top in -8_i64..8,
        selection_width in 1_u32..9,
        selection_height in 1_u32..9,
        initial in color(),
        fill in color(),
    ) {
        let size = Size::new(width, height).unwrap();
        let right = left + i64::from(selection_width);
        let bottom = top + i64::from(selection_height);
        let selection = Rect::new(left, top, right, bottom).unwrap();
        let clip = selection.clip_to(size);
        let mut session = session(size, initial);
        let original = session.document().layers()[0].surface().pixels().to_vec();

        session.execute(Command::SelectRect(selection)).unwrap();
        let changed = session.execute(Command::Fill(fill)).unwrap();
        let expected_changed = clip.is_some_and(|rect| {
            (0..height).any(|y| (0..width).any(|x| {
                rect.contains(Point::new(x, y)) && original[(y * width + x) as usize] != fill
            }))
        });
        prop_assert_eq!(changed, expected_changed);

        let expected_after: Vec<Bgra> = (0..height)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .map(|(x, y)| {
                let index = (y * width + x) as usize;
                if clip.is_some_and(|rect| rect.contains(Point::new(x, y))) {
                    fill
                } else {
                    original[index]
                }
            })
            .collect();
        prop_assert_eq!(session.document().layers()[0].surface().pixels(), expected_after.as_slice());
        prop_assert_eq!(session.dirty(), changed);

        if changed {
            for _ in 0..3 {
                session.undo().unwrap();
                prop_assert_eq!(session.document().layers()[0].surface().pixels(), original.as_slice());
                prop_assert!(!session.dirty());
                session.redo().unwrap();
                prop_assert_eq!(session.document().layers()[0].surface().pixels(), expected_after.as_slice());
                prop_assert!(session.dirty());
            }
        } else {
            prop_assert!(!session.can_undo());
            prop_assert!(!session.can_redo());
        }
    }
}
