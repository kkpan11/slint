//! Inline each object_tree::Component within the main Component

use crate::{
    expression_tree::{Expression, NamedReference},
    object_tree::*,
    typeregister::Type,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub fn inline(doc: &Document) {
    fn inline_components_recursively(component: &Rc<Component>) {
        recurse_elem(&component.root_element, &(), &mut |elem, _| {
            let base = elem.borrow().base_type.clone();
            if let Type::Component(c) = base {
                // First, make sure that the component itself is properly inlined
                inline_components_recursively(&c);
                // Inline this component.
                inline_element(elem, &c, component);
            }
        })
    }
    inline_components_recursively(&doc.root_component)
}

fn clone_tuple<U: Clone, V: Clone>((u, v): (&U, &V)) -> (U, V) {
    (u.clone(), v.clone())
}

fn element_key(e: &ElementRc) -> usize {
    &**e as *const RefCell<Element> as usize
}

fn inline_element(
    elem: &ElementRc,
    inlined_component: &Rc<Component>,
    root_component: &Rc<Component>,
) {
    // inlined_component must be the base type of this element
    debug_assert_eq!(
        format!("{:?}", elem.borrow().base_type),
        format!("{:?}", Type::Component(inlined_component.clone()))
    );
    debug_assert!(
        inlined_component.root_element.borrow().repeated.is_none(),
        "root element of a component cannot be repeated"
    );

    let mut elem_mut = elem.borrow_mut();
    elem_mut.base_type = inlined_component.root_element.borrow().base_type.clone();
    elem_mut.property_declarations.extend(
        inlined_component.root_element.borrow().property_declarations.iter().map(clone_tuple),
    );
    elem_mut.property_animations.extend(
        inlined_component.root_element.borrow().property_animations.iter().map(clone_tuple),
    );

    // Map the old element to the new
    let mut mapping = HashMap::new();
    mapping.insert(element_key(&inlined_component.root_element), elem.clone());

    let mut new_children = vec![];
    new_children
        .reserve(elem_mut.children.len() + inlined_component.root_element.borrow().children.len());
    new_children.extend(
        inlined_component
            .root_element
            .borrow()
            .children
            .iter()
            .map(|x| duplicate_element_with_mapping(x, &mut mapping, root_component)),
    );
    new_children.append(&mut elem_mut.children);
    elem_mut.children = new_children;

    elem_mut.bindings.extend(
        inlined_component
            .root_element
            .borrow()
            .bindings
            .iter()
            .map(|(k, val)| (k.clone(), fold_binding(val, &mapping))),
    );

    //core::mem::drop(elem_mut);

    // Now fixup all binding and reference
    for (key, e) in &mapping {
        if *key == element_key(&inlined_component.root_element) {
            continue; // the root has been processed
        }
        for (_, expr) in &mut e.borrow_mut().bindings {
            fixup_binding(expr, &mapping);
        }
        if let Some(ref mut r) = &mut e.borrow_mut().repeated {
            fixup_binding(&mut r.model, &mapping);
        }
        for s in &mut e.borrow_mut().states {
            if let Some(cond) = s.condition.as_mut() {
                fixup_binding(cond, &mapping)
            }
            for (r, e) in &mut s.property_changes {
                fixup_reference(r, &mapping);
                fixup_binding(e, &mapping);
            }
        }
        for t in &mut e.borrow_mut().transitions {
            for (r, _) in &mut t.property_animations {
                fixup_reference(r, &mapping)
            }
        }
    }
}

// Duplicate the element elem and all its children. And fill the mapping to point from the old to the new
fn duplicate_element_with_mapping(
    element: &ElementRc,
    mapping: &mut HashMap<usize, ElementRc>,
    root_component: &Rc<Component>,
) -> ElementRc {
    let elem = element.borrow();
    let new = Rc::new(RefCell::new(Element {
        base_type: elem.base_type.clone(),
        id: elem.id.clone(),
        property_declarations: elem.property_declarations.clone(),
        property_animations: elem
            .property_animations
            .iter()
            .map(|(k, v)| (k.clone(), duplicate_element_with_mapping(v, mapping, root_component)))
            .collect(),
        // We will do the fixup of the bindings later
        bindings: elem.bindings.clone(),
        children: elem
            .children
            .iter()
            .map(|x| duplicate_element_with_mapping(x, mapping, root_component))
            .collect(),
        repeated: elem.repeated.clone(),
        node: elem.node.clone(),
        enclosing_component: Rc::downgrade(root_component),
        states: elem.states.clone(),
        transitions: elem
            .transitions
            .iter()
            .map(|t| duplicate_transition(t, mapping, root_component))
            .collect(),
    }));
    mapping.insert(element_key(element), new.clone());
    new
}

fn fixup_reference(
    NamedReference { element, .. }: &mut NamedReference,
    mapping: &HashMap<usize, ElementRc>,
) {
    *element =
        element.upgrade().and_then(|e| mapping.get(&element_key(&e))).map(Rc::downgrade).unwrap();
}

fn fixup_binding(val: &mut Expression, mapping: &HashMap<usize, ElementRc>) {
    val.visit_mut(|sub| fixup_binding(sub, mapping));
    match val {
        Expression::PropertyReference(r) => fixup_reference(r, mapping),
        Expression::SignalReference(r) => fixup_reference(r, mapping),
        _ => {}
    }
}

fn fold_binding(val: &Expression, mapping: &HashMap<usize, ElementRc>) -> Expression {
    let mut new_val = val.clone();
    fixup_binding(&mut new_val, mapping);
    new_val
}

fn duplicate_transition(
    t: &Transition,
    mapping: &mut HashMap<usize, Rc<RefCell<Element>>>,
    root_component: &Rc<Component>,
) -> Transition {
    Transition {
        is_out: t.is_out,
        state_id: t.state_id.clone(),
        property_animations: t
            .property_animations
            .iter()
            .map(|(r, anim)| {
                (r.clone(), duplicate_element_with_mapping(anim, mapping, root_component))
            })
            .collect(),
    }
}
