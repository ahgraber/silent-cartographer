use crate::shared::{SharedTr, SharedType};
use crate::traits::{Dflt, Edge, Generated, Made, Named, Remote, Req, Tr};
use crate::widget::{Gadget, Widget};

/// Calls the trait methods through generic bounds, so the references land on the trait declarations.
pub fn call_generic<R: Req + Dflt, T: Tr + Remote + Made + Named + Generated, E: Edge, S: SharedTr>(
    req: &R,
    tr: &T,
    edge: &E,
    shared: &S,
) -> u32 {
    req.req()
        + req.dm()
        + tr.tr().len() as u32
        + tr.remote()
        + tr.made()
        + tr.generated()
        + tr.name().len() as u32
        + edge.edge()
        + shared.shared()
}

pub fn call_all(widget: &Widget, gadget: &Gadget) -> u32 {
    let mut total = widget.req() + gadget.req();
    total += widget.dm() + gadget.dm();
    total += widget.name().len() as u32 + Named::name(widget).len() as u32;
    total += widget.tr().len() as u32;
    total += widget.edge() + (1u8, 2u8).edge();
    total += gadget.made() + gadget.remote() + gadget.generated();
    total += SharedType.shared();
    total
}
