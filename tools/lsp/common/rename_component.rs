// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

use std::path::Path;

use crate::{common, util};

use i_slint_compiler::{
    diagnostics::{SourceFile, Spanned},
    object_tree,
    parser::{syntax_nodes, SyntaxKind},
    typeloader::TypeLoader,
};

fn symbol_export_names(document_node: &syntax_nodes::Document, type_name: &str) -> Vec<String> {
    let mut result = vec![];

    for export in document_node.ExportsList() {
        for specifier in export.ExportSpecifier() {
            if specifier.ExportIdentifier().text() == type_name {
                result.push(
                    specifier
                        .ExportName()
                        .map(|n| n.text().to_string())
                        .unwrap_or_else(|| type_name.to_string()),
                );
            }
        }
        if let Some(component) = export.Component() {
            if component.DeclaredIdentifier().text() == type_name {
                result.push(type_name.to_string());
            }
        }
        for structs in export.StructDeclaration() {
            if structs.DeclaredIdentifier().text() == type_name {
                result.push(type_name.to_string());
            }
        }
        for enums in export.EnumDeclaration() {
            if enums.DeclaredIdentifier().text() == type_name {
                result.push(type_name.to_string());
            }
        }
    }

    result.sort();
    result
}

fn replace_element_types(
    element: &syntax_nodes::Element,
    old_type: &str,
    new_type: &str,
    edits: &mut Vec<(SourceFile, lsp_types::TextEdit)>,
) {
    if let Some(name) = element.QualifiedName() {
        if name.text().to_string().trim() == old_type {
            edits.push((
                element.source_file.clone(),
                lsp_types::TextEdit {
                    range: util::map_node(&name).expect("Just had a source_file"),
                    new_text: new_type.to_string(),
                },
            ))
        }
    }

    for c in element.children() {
        match c.kind() {
            SyntaxKind::SubElement => {
                let e: syntax_nodes::SubElement = c.into();
                replace_element_types(&e.Element(), old_type, new_type, edits);
            }
            SyntaxKind::RepeatedElement => {
                let e: syntax_nodes::RepeatedElement = c.into();
                replace_element_types(&e.SubElement().Element(), old_type, new_type, edits);
            }
            SyntaxKind::ConditionalElement => {
                let e: syntax_nodes::ConditionalElement = c.into();
                replace_element_types(&e.SubElement().Element(), old_type, new_type, edits);
            }
            _ => { /* do nothing */ }
        }
    }
}

fn fix_imports(
    type_loader: &TypeLoader,
    exporter_path: &Path,
    old_type: &str,
    new_type: &str,
    edits: &mut Vec<(SourceFile, lsp_types::TextEdit)>,
) {
    for doc in type_loader.all_documents() {
        let Some(doc_path) =
            doc.node.as_ref().and_then(|n| n.source_file()).map(|sf| sf.path().to_owned())
        else {
            continue;
        };
        if doc_path.starts_with("builtin:") {
            continue;
        }
        if doc_path == exporter_path {
            continue;
        }
        fix_import_in_document(type_loader, doc, exporter_path, old_type, new_type, edits);
    }
}

fn fix_import_in_document(
    type_loader: &TypeLoader,
    document: &object_tree::Document,
    exporter_path: &Path,
    old_type: &str,
    new_type: &str,
    edits: &mut Vec<(SourceFile, lsp_types::TextEdit)>,
) {
    let Some(document_node) = &document.node else {
        return;
    };

    let Some(document_directory) =
        document_node.source_file().and_then(|sf| sf.path().parent()).map(|p| p.to_owned())
    else {
        return;
    };

    for import_specifier in document_node.ImportSpecifier() {
        let import = import_specifier
            .child_token(SyntaxKind::StringLiteral)
            .map(|t| t.text().trim_matches('"').to_string())
            .unwrap_or_default();

        // Do not bother with the TypeLoader: It will check the FS, which we do not use:-/
        let import_path = document_directory.join(import);

        if import_path != exporter_path {
            continue;
        }

        let Some(list) = import_specifier.ImportIdentifierList() else {
            continue;
        };

        for identifier in list.ImportIdentifier() {
            let external = identifier.ExternalName();

            if external.text().to_string().trim() != old_type {
                continue;
            }

            let Some(source_file) = external.source_file() else {
                continue;
            };

            edits.push((
                source_file.clone(),
                lsp_types::TextEdit {
                    range: util::map_node(&external).expect("Just had a source file"),
                    new_text: new_type.to_string(),
                },
            ));

            if let Some(internal) = identifier.InternalName() {
                let internal_name = internal.text().to_string().trim().to_string();
                if internal_name == new_type {
                    // remove " as Foo" part, no need to change anything else though!
                    let start_position =
                        util::map_position(source_file, external.text_range().end());
                    let end_position =
                        util::map_position(source_file, identifier.text_range().end());
                    edits.push((
                        source_file.clone(),
                        lsp_types::TextEdit {
                            range: lsp_types::Range::new(start_position, end_position),
                            new_text: String::new(),
                        },
                    ));
                }
                // Nothing else to change: We still use the old internal name.
                continue;
            }

            // Change exports
            fix_exports(type_loader, document_node, old_type, new_type, edits);

            // Change all local usages:
            change_local_element_type(document_node, old_type, new_type, edits);
        }
    }
}

fn change_local_element_type(
    document_node: &syntax_nodes::Document,
    old_type: &str,
    new_type: &str,
    edits: &mut Vec<(SourceFile, lsp_types::TextEdit)>,
) {
    for component in document_node.Component() {
        replace_element_types(&component.Element(), old_type, new_type, edits);
    }
    for exported in document_node.ExportsList() {
        if let Some(component) = exported.Component() {
            replace_element_types(&component.Element(), old_type, new_type, edits);
        }
    }
}

fn fix_exports(
    type_loader: &TypeLoader,
    document_node: &syntax_nodes::Document,
    old_type: &str,
    new_type: &str,
    edits: &mut Vec<(SourceFile, lsp_types::TextEdit)>,
) {
    for export in document_node.ExportsList() {
        for specifier in export.ExportSpecifier() {
            let identifier = specifier.ExportIdentifier();
            if identifier.text().to_string().trim() == old_type {
                let Some(source_file) = identifier.source_file() else {
                    continue;
                };
                edits.push((
                    source_file.clone(),
                    lsp_types::TextEdit {
                        range: util::map_node(&identifier)
                            .expect("This needs to have a source file"),
                        new_text: new_type.to_string(),
                    },
                ));

                let update_imports = if let Some(export_name) = specifier.ExportName() {
                    // Remove "as Foo"
                    if export_name.text().to_string().trim() == new_type {
                        let start_position =
                            util::map_position(source_file, identifier.text_range().end());
                        let end_position =
                            util::map_position(source_file, export_name.text_range().end());
                        edits.push((
                            source_file.clone(),
                            lsp_types::TextEdit {
                                range: lsp_types::Range::new(start_position, end_position),
                                new_text: String::new(),
                            },
                        ));
                        true
                    } else {
                        false
                    }
                } else {
                    true
                };

                if update_imports {
                    let my_path = document_node.source_file.path().to_owned();
                    fix_imports(type_loader, &my_path, old_type, new_type, edits);
                }
            }
        }
    }
}

/// Rename a component by providing the `DeclaredIdentifier` in the component definition.
pub fn rename_component_from_definition(
    type_loader: &TypeLoader,
    identifier: &syntax_nodes::DeclaredIdentifier,
    new_name: String,
) -> crate::Result<lsp_types::WorkspaceEdit> {
    let source_file = identifier.source_file().expect("Identifier had no source file");
    let document =
        type_loader.get_document(source_file.path()).expect("Identifier is in unknown document");

    if document.local_registry.lookup(&new_name) != i_slint_compiler::langtype::Type::Invalid {
        return Err(format!("{new_name} is already a registered type").into());
    }
    if document.local_registry.lookup_element(&new_name).is_ok() {
        return Err(format!("{new_name} is already a registered element").into());
    }

    let component_type = identifier.text().to_string().trim().to_string();
    if component_type == new_name {
        return Ok(lsp_types::WorkspaceEdit::default());
    }

    let component = identifier.parent().expect("Identifier had no parent");
    debug_assert_eq!(component.kind(), SyntaxKind::Component);

    let Some(document_node) = &document.node else {
        return Err("No document found".into());
    };

    let mut edits = vec![];

    // Replace the identifier itself
    edits.push((
        source_file.clone(),
        lsp_types::TextEdit {
            range: util::map_node(identifier).expect("This has a source_file"),
            new_text: new_name.clone(),
        },
    ));

    // Change all local usages:
    change_local_element_type(document_node, &component_type, &new_name, &mut edits);

    // Change exports
    fix_exports(type_loader, document_node, &component_type, &new_name, &mut edits);

    let export_names = symbol_export_names(document_node, &component_type);
    if export_names.contains(&component_type) {
        let my_path = source_file.path().to_owned();

        fix_imports(type_loader, &my_path, &component_type, &new_name, &mut edits);
    }

    common::create_workspace_edit_from_source_files(edits)
        .ok_or("Failed to create workspace edit".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;
    use std::path::PathBuf;

    use crate::common::test;
    use crate::common::text_edit;

    #[track_caller]
    fn compile_test_changes(
        type_loader: &TypeLoader,
        edit: &lsp_types::WorkspaceEdit,
    ) -> Vec<text_edit::EditedText> {
        eprintln!("Edit:");
        for it in text_edit::EditIterator::new(edit) {
            eprintln!("   {} => {:?}", it.0.uri.to_string(), it.1);
        }
        eprintln!("*** All edits reported ***");

        let changed_text = text_edit::apply_workspace_edit(&type_loader, &edit).unwrap();
        assert!(!changed_text.is_empty()); // there was a change!

        eprintln!("After changes were applied:");
        for ct in &changed_text {
            eprintln!("File {}:", ct.url.to_string());
            for (count, line) in ct.contents.split('\n').enumerate() {
                eprintln!("    {:3}: {line}", count + 1);
            }
            eprintln!("=========");
        }
        eprintln!("*** All changes reported ***");

        let code = {
            let mut map: HashMap<PathBuf, String> = type_loader
                .all_documents()
                .filter_map(|doc| doc.node.as_ref())
                .map(|dn| dn.source_file.as_ref())
                .map(|sf| (sf.path().to_owned(), sf.source().unwrap().to_string()))
                .collect();
            for ct in &changed_text {
                map.insert(ct.url.to_file_path().unwrap(), ct.contents.clone());
            }
            map
        };

        // changed code compiles fine:
        let _ = test::recompile_test_with_sources("fluent", code);

        changed_text
    }

    fn find_component_declared_identifier(
        document_node: &syntax_nodes::Document,
        name: &str,
    ) -> Option<syntax_nodes::DeclaredIdentifier> {
        for c in document_node.Component() {
            let cid = c.DeclaredIdentifier();
            if cid.text().to_string().trim() == name {
                return Some(cid);
            }
        }
        for e in document_node.ExportsList() {
            if let Some(c) = e.Component() {
                let cid = c.DeclaredIdentifier();
                if cid.text().to_string().trim() == name {
                    return Some(cid);
                }
            }
        }
        None
    }

    #[test]
    fn test_rename_component_from_definition_ok() {
        let type_loader = test::compile_test_with_sources(
            "fluent",
            HashMap::from([(
                test::main_test_file_name(),
                r#"
component Foo { }

component Baz {
    Foo { }
}

export component Bar {
    Foo { }
    Rectangle {
        Foo { }
        Baz { }
    }

    if true: Rectangle {
        Foo { }
    }

    for i in [1, 2, 3]: Foo { }
}
                    "#
                .to_string(),
            )]),
        );

        let doc = type_loader.get_document(&test::main_test_file_name()).unwrap();

        let foo_identifier =
            find_component_declared_identifier(doc.node.as_ref().unwrap(), "Foo").unwrap();
        let edit = rename_component_from_definition(
            &type_loader,
            &foo_identifier,
            "XxxYyyZzz".to_string(),
        )
        .unwrap();

        let edited_text = compile_test_changes(&type_loader, &edit);

        assert_eq!(edited_text.len(), 1);
        assert!(edited_text[0].contents.contains("XxxYyyZzz"));
        assert!(!edited_text[0].contents.contains("Foo"));
    }

    #[test]
    fn test_rename_component_from_definition_with_renaming_export_ok() {
        let type_loader = test::compile_test_with_sources(
            "fluent",
            HashMap::from([
                (
                    test::main_test_file_name(),
                    r#"
import { FExport} from "source.slint";

export component Foo {
    FExport { }
}
                "#
                    .to_string(),
                ),
                (
                    test::test_file_name("source.slint"),
                    r#"
component Foo { }

export { Foo as FExport }
                "#
                    .to_string(),
                ),
            ]),
        );

        let doc = type_loader.get_document(&test::test_file_name("source.slint")).unwrap();

        let foo_identifier =
            find_component_declared_identifier(doc.node.as_ref().unwrap(), "Foo").unwrap();
        let edit = rename_component_from_definition(
            &type_loader,
            &foo_identifier,
            "XxxYyyZzz".to_string(),
        )
        .unwrap();

        let edited_text = compile_test_changes(&type_loader, &edit);

        assert_eq!(edited_text.len(), 1);
        assert_eq!(
            edited_text[0].url.to_file_path().unwrap(),
            test::test_file_name("source.slint")
        );
        assert!(edited_text[0].contents.contains("XxxYyyZzz"));
        assert!(!edited_text[0].contents.contains("Foo"));
    }

    #[test]
    fn test_rename_component_from_definition_with_export_ok() {
        let type_loader = test::compile_test_with_sources(
            "fluent",
            HashMap::from([
                (
                    test::main_test_file_name(),
                    r#"
import { Foo } from "source.slint";
import { UserComponent } from "user.slint";
import { User2Component } from "user2.slint";
import { Foo as User3Fxx } from "user3.slint";
import { User4Fxx } from "user4.slint";

export component Main {
    Foo { }
    UserComponent { }
    User2Component { }
}
                "#
                    .to_string(),
                ),
                (
                    test::test_file_name("source.slint"),
                    r#"
export component Foo { }
                "#
                    .to_string(),
                ),
                (
                    test::test_file_name("user.slint"),
                    r#"
import { Foo as Bar } from "source.slint";

export component UserComponent { 
    Bar { }
}

export { Bar }
                "#
                    .to_string(),
                ),
                (
                    test::test_file_name("user2.slint"),
                    r#"
import { Foo as XxxYyyZzz } from "source.slint";

export component User2Component { 
    XxxYyyZzz { }
}
                "#
                    .to_string(),
                ),
                (
                    test::test_file_name("user3.slint"),
                    r#"
import { Foo } from "source.slint";

export { Foo }
                "#
                    .to_string(),
                ),
                (
                    test::test_file_name("user4.slint"),
                    r#"
import { Foo } from "source.slint";

export { Foo as User4Fxx }
                "#
                    .to_string(),
                ),
            ]),
        );

        let doc = type_loader.get_document(&test::test_file_name("source.slint")).unwrap();

        let foo_identifier =
            find_component_declared_identifier(doc.node.as_ref().unwrap(), "Foo").unwrap();
        let edit = rename_component_from_definition(
            &type_loader,
            &foo_identifier,
            "XxxYyyZzz".to_string(),
        )
        .unwrap();

        let edited_text = compile_test_changes(&type_loader, &edit);

        for ed in &edited_text {
            let ed_path = ed.url.to_file_path().unwrap();
            if ed_path == test::main_test_file_name() {
                assert!(ed.contents.contains("XxxYyyZzz"));
                assert!(!ed.contents.contains("Foo"));
                assert!(ed.contents.contains("UserComponent"));
                assert!(ed.contents.contains("import { XxxYyyZzz as User3Fxx }"));
                assert!(ed.contents.contains("import { User4Fxx }"));
            } else if ed_path == test::test_file_name("source.slint") {
                assert!(ed.contents.contains("export component XxxYyyZzz {"));
                assert!(!ed.contents.contains("Foo"));
            } else if ed_path == test::test_file_name("user.slint") {
                assert!(ed.contents.contains("{ XxxYyyZzz as Bar }"));
                assert!(ed.contents.contains("Bar { }"));
                assert!(!ed.contents.contains("Foo"));
            } else if ed_path == test::test_file_name("user2.slint") {
                assert!(ed.contents.contains("import { XxxYyyZzz }"));
                assert!(ed.contents.contains("XxxYyyZzz { }"));
            } else if ed_path == test::test_file_name("user3.slint") {
                assert!(ed.contents.contains("import { XxxYyyZzz }"));
                assert!(ed.contents.contains("export { XxxYyyZzz }"));
            } else if ed_path == test::test_file_name("user4.slint") {
                assert!(ed.contents.contains("import { XxxYyyZzz }"));
                assert!(ed.contents.contains("export { XxxYyyZzz as User4Fxx }"));
            } else {
                unreachable!();
            }
        }
    }

    #[test]
    fn test_rename_component_from_definition_import_confusion_ok() {
        let type_loader = test::compile_test_with_sources(
            "fluent",
            HashMap::from([
                (
                    test::main_test_file_name(),
                    r#"
import { Foo as User1Fxx } from "user1.slint";
import { Foo as User2Fxx } from "user2.slint";

export component Main {
    User1Fxx { }
    User2Fxx { }
}
                "#
                    .to_string(),
                ),
                (
                    test::test_file_name("user1.slint"),
                    r#"
export component Foo { }
                "#
                    .to_string(),
                ),
                (
                    test::test_file_name("user2.slint"),
                    r#"
export component Foo { }
                "#
                    .to_string(),
                ),
            ]),
        );

        let doc = type_loader.get_document(&test::test_file_name("user1.slint")).unwrap();

        let foo_identifier =
            find_component_declared_identifier(doc.node.as_ref().unwrap(), "Foo").unwrap();
        let edit = rename_component_from_definition(
            &type_loader,
            &foo_identifier,
            "XxxYyyZzz".to_string(),
        )
        .unwrap();

        let edited_text = compile_test_changes(&type_loader, &edit);

        for ed in &edited_text {
            let ed_path = ed.url.to_file_path().unwrap();
            if ed_path == test::main_test_file_name() {
                assert!(ed.contents.contains("import { XxxYyyZzz as User1Fxx }"));
                assert!(ed.contents.contains("import { Foo as User2Fxx }"));
            } else if ed_path == test::test_file_name("user1.slint") {
                assert!(ed.contents.contains("export component XxxYyyZzz {"));
                assert!(!ed.contents.contains("Foo"));
            } else {
                unreachable!();
            }
        }

        let doc = type_loader.get_document(&test::test_file_name("user2.slint")).unwrap();

        let foo_identifier =
            find_component_declared_identifier(doc.node.as_ref().unwrap(), "Foo").unwrap();
        let edit = rename_component_from_definition(
            &type_loader,
            &foo_identifier,
            "XxxYyyZzz".to_string(),
        )
        .unwrap();

        let edited_text = compile_test_changes(&type_loader, &edit);

        for ed in &edited_text {
            let ed_path = ed.url.to_file_path().unwrap();
            if ed_path == test::main_test_file_name() {
                assert!(ed.contents.contains("import { XxxYyyZzz as User2Fxx }"));
                assert!(ed.contents.contains("import { Foo as User1Fxx }"));
            } else if ed_path == test::test_file_name("user2.slint") {
                assert!(ed.contents.contains("export component XxxYyyZzz {"));
                assert!(!ed.contents.contains("Foo"));
            } else {
                unreachable!();
            }
        }
    }

    #[test]
    fn test_rename_component_from_definition_redefinition_error() {
        let type_loader = test::compile_test_with_sources(
            "fluent",
            HashMap::from([(
                test::main_test_file_name(),
                r#"
struct UsedStruct { value: int, }
enum UsedEnum { x, y }

component Foo { }

component Baz {
    Foo { }
}

export component Bar {
    Foo { }
    Rectangle {
        Foo { }
        Baz { }
    }
}
                    "#
                .to_string(),
            )]),
        );

        let doc = type_loader.get_document(&test::main_test_file_name()).unwrap();

        let foo_identifier =
            find_component_declared_identifier(doc.node.as_ref().unwrap(), "Foo").unwrap();

        assert!(rename_component_from_definition(&type_loader, &foo_identifier, "Foo".to_string())
            .is_err());
        assert!(rename_component_from_definition(
            &type_loader,
            &foo_identifier,
            "UsedStruct".to_string()
        )
        .is_err());
        assert!(rename_component_from_definition(
            &type_loader,
            &foo_identifier,
            "UsedEnum".to_string()
        )
        .is_err());
        assert!(rename_component_from_definition(&type_loader, &foo_identifier, "Baz".to_string())
            .is_err());
        assert!(rename_component_from_definition(
            &type_loader,
            &foo_identifier,
            "HorizontalLayout".to_string()
        )
        .is_err());
    }

    #[test]
    fn test_exported_type_names() {
        let type_loader = test::compile_test_with_sources(
            "fluent",
            HashMap::from([(
                test::main_test_file_name(),
                r#"
export component Foo {}
export component Baz {}

component Bar {}
component Bat {}

export { Bat, Bar as RenamedBar, Baz as RenamedBaz, StructBar as RenamedStructBar }

export struct StructBar { foo: int }

export enum EnumBar { bar }
                    "#
                .to_string(),
            )]),
        );

        let doc = type_loader.get_document(&test::main_test_file_name()).unwrap();
        let doc = doc.node.as_ref().unwrap();

        assert!(symbol_export_names(doc, "Foobar").is_empty());
        assert_eq!(symbol_export_names(doc, "Foo"), vec!["Foo".to_string()]);
        assert_eq!(
            symbol_export_names(doc, "Baz"),
            vec!["Baz".to_string(), "RenamedBaz".to_string()]
        );
        assert_eq!(symbol_export_names(doc, "Bar"), vec!["RenamedBar".to_string()]);
        assert_eq!(symbol_export_names(doc, "Bat"), vec!["Bat".to_string()]);
        assert_eq!(
            symbol_export_names(doc, "StructBar"),
            vec!["RenamedStructBar".to_string(), "StructBar".to_string()]
        );
        assert_eq!(symbol_export_names(doc, "EnumBar"), vec!["EnumBar".to_string()]);
    }
}