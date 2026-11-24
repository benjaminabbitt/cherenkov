//! As code generated into the build: a type named like one in the crate's
//! root, with a Default the program uses.

#[derive(Default, Debug, PartialEq)]
pub struct Letter {
    pub body: Vec<u8>,
    pub id: u64,
}
