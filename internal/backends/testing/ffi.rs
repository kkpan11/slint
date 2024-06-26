// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

use i_slint_core::item_tree::ItemTreeRc;
use i_slint_core::slice::Slice;
use i_slint_core::{SharedString, SharedVector};

#[no_mangle]
pub extern "C" fn slint_testing_init_backend() {
    crate::init_integration_test();
}

#[no_mangle]
pub extern "C" fn slint_testing_element_find_by_accessible_label(
    root: &ItemTreeRc,
    label: &Slice<u8>,
    out: &mut SharedVector<crate::search_api::ElementHandle>,
) {
    let Ok(label) = core::str::from_utf8(label.as_slice()) else { return };
    *out = crate::search_api::search_item(root, |elem| {
        elem.accessible_label().is_some_and(|x| x == label)
    })
}

#[no_mangle]
pub extern "C" fn slint_testing_element_find_by_element_id(
    root: &ItemTreeRc,
    element_id: &Slice<u8>,
    out: &mut SharedVector<crate::search_api::ElementHandle>,
) {
    let Ok(element_id) = core::str::from_utf8(element_id.as_slice()) else { return };
    out.extend(crate::ElementHandle::find_by_element_id_with_tree(&root, element_id));
}

#[no_mangle]
pub extern "C" fn slint_testing_element_find_by_element_type_name(
    root: &ItemTreeRc,
    type_name: &Slice<u8>,
    out: &mut SharedVector<crate::search_api::ElementHandle>,
) {
    let Ok(type_name) = core::str::from_utf8(type_name.as_slice()) else { return };
    out.extend(crate::ElementHandle::find_by_element_type_name_with_tree(&root, type_name));
}

#[no_mangle]
pub extern "C" fn slint_testing_element_id(
    element: &crate::search_api::ElementHandle,
    out: &mut SharedString,
) -> bool {
    if let Some(id) = element.id() {
        *out = id;
        true
    } else {
        false
    }
}

#[no_mangle]
pub extern "C" fn slint_testing_element_type_name(
    element: &crate::search_api::ElementHandle,
    out: &mut SharedString,
) -> bool {
    if let Some(type_name) = element.type_name() {
        *out = type_name;
        true
    } else {
        false
    }
}

#[no_mangle]
pub extern "C" fn slint_testing_element_bases(
    element: &crate::search_api::ElementHandle,
    out: &mut SharedVector<SharedString>,
) -> bool {
    if let Some(bases_it) = element.bases() {
        out.extend(bases_it);
        true
    } else {
        false
    }
}
