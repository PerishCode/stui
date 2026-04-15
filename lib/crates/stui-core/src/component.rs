#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ComponentId(pub &'static str);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentDescriptor {
    pub id: ComponentId,
    pub role: &'static str,
    pub parent: Option<ComponentId>,
}

impl ComponentDescriptor {
    pub const fn root(id: &'static str, role: &'static str) -> Self {
        Self {
            id: ComponentId(id),
            role,
            parent: None,
        }
    }

    pub const fn child(id: &'static str, role: &'static str, parent: &'static str) -> Self {
        Self {
            id: ComponentId(id),
            role,
            parent: Some(ComponentId(parent)),
        }
    }
}
