use paint_app::{AppSession, Command};
use paint_core::{Bgra, Size};
use paint_document::Document;

#[test]
fn pdn_save_is_atomic_layered_and_marks_the_document_clean() {
    let mut session = AppSession::new(Document::new(Size::new(2, 2).unwrap()).unwrap());
    session
        .execute(Command::AddLayer {
            name: "Ink".to_owned(),
            opacity: 200,
            visible: true,
        })
        .unwrap();
    session
        .execute(Command::Fill(Bgra::new(0, 0, 255, 255)))
        .unwrap();
    assert!(session.dirty());

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("layered.pdn");
    session.save_pdn_atomic(&path).unwrap();

    assert!(!session.dirty());
    let bytes = std::fs::read(&path).unwrap();
    let decoded = paint_pdn3::decode_pdn(&bytes).expect("decode saved PDN");
    assert_eq!(decoded.document.layers().len(), 2);
    assert_eq!(decoded.document.layers()[1].name(), "Ink");
    assert_eq!(decoded.document.layers()[1].opacity(), 200);
    assert_eq!(
        session.document().layers()[1]
            .surface()
            .pixel(paint_core::Point::new(0, 0))
            .unwrap(),
        &Bgra::new(0, 0, 255, 255)
    );
}
