use std::num::NonZeroU16;
use tree_sitter::{Node, TreeCursor};

pub struct Context<'a> {
    cursor: TreeCursor<'a>,
    traversed: bool,
}

impl Context<'_> {
    #[inline(always)]
    pub fn node(&self) -> Node {
        self.cursor.node()
    }

    #[inline(always)]
    pub fn field_name(&self) -> Option<&str> {
        self.cursor.field_name()
    }

    #[inline(always)]
    pub fn field_id(&self) -> Option<NonZeroU16> {
        self.cursor.field_id()
    }

    #[inline(always)]
    pub fn traversed(&self) -> bool {
        self.traversed
    }
}

macro_rules! methods {
    ($($name:ident),+ $(,)?) => {
        $(
            #[allow(unused_variables)]
            #[inline(always)]
            fn $name(&mut self, context: &mut Context) -> Result {
                Ok(())
            }
        )+
    };
}

pub type Result = anyhow::Result<()>;

pub trait Visitor {
    methods!(
        on_root,    // Happens once on a root node.
        on_end,     // Happens once after all tree traversal.
        on_child,   // Happens on every descent to a child node.
        on_leaf,    // Happens only on nodes that doesn't have children.
        on_sibling, // Happens on every move to a sibling node.
        on_parent,  // Happens on return to a parent node from a child node.
        on_visit,   // Happens on every node after all above events and triggers
                    //   twice for every node except the root node. The first time
                    //   on forward direction and the second on returning direction
                    //   like after on_leaf() or on_parent() events.

                    // Event chains possible on a one node.
                    // The first event in chains doesn't exist on the root node,
                    // but instead for it a one time `on_root()` event triggers.
                    //
                    //   on_child()   -> on_visit() -> on_leaf()   -> on_visit()
                    //   on_child()   -> on_visit() -> on_parent() -> on_visit()
                    //   on_sibling() -> on_visit() -> on_leaf()   -> on_visit()
                    //   on_sibling() -> on_visit() -> on_parent() -> on_visit()
    );

    fn perform(&mut self, cursor: TreeCursor) -> Result {
        let mut c = Context {
            traversed: false,
            cursor,
        };
        // Traverse logic -----------------------------------
        self.on_root(&mut c)?;
        loop {
            if !c.traversed {
                if c.cursor.goto_first_child() {
                    c.traversed = false;
                    self.on_child(&mut c)?;
                } else {
                    c.traversed = true;
                    self.on_leaf(&mut c)?;
                }
            } else {
                if c.cursor.goto_next_sibling() {
                    c.traversed = false;
                    self.on_sibling(&mut c)?;
                } else if c.cursor.goto_parent() {
                    c.traversed = true;
                    self.on_parent(&mut c)?;
                } else {
                    break;
                }
            }
            self.on_visit(&mut c)?;
        }
        self.on_end(&mut c)?;
        //---------------------------------------------------
        Ok(())
    }
}
