#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EventId(pub &'static str);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventDescriptor {
    pub id: EventId,
    pub detail: &'static str,
}

impl EventDescriptor {
    pub const fn new(id: &'static str, detail: &'static str) -> Self {
        Self {
            id: EventId(id),
            detail,
        }
    }
}
