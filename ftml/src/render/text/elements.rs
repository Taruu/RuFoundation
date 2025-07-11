/*
 * render/text/elements.rs
 *
 * ftml - Library to parse Wikidot text
 * Copyright (C) 2019-2022 Wikijump Team
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
 * GNU Affero General Public License for more details.
 *
 * You should have received a copy of the GNU Affero General Public License
 * along with this program. If not, see <http://www.gnu.org/licenses/>.
 */

//! Module that implements text rendering for `Element` and its children.

use super::TextContext;
use crate::tree::{
    ContainerType, DefinitionListItem, Element, LinkLocation, ListItem, ListType, Tab, FormInput
};
use crate::url::normalize_link;
use std::borrow::Cow;

pub fn render_elements(ctx: &mut TextContext, elements: &[Element]) {
    info!("Rendering elements (length {})", elements.len());

    for element in elements {
        render_element(ctx, element);
    }
}

pub fn render_element(ctx: &mut TextContext, element: &Element) {
    info!("Rendering element {}", element.name());

    match element {
        Element::Fragment(elements) => render_elements(ctx, elements),
        Element::AlignMarker(_) => {},
        Element::Container(container) => {
            let mut invisible = false;
            let (add_newlines, prefix) = match container.ctype() {
                // Don't render this at all.
                ContainerType::Hidden => return,

                // Render it, but invisibly.
                // Requires setting a special mode in the context.
                ContainerType::Invisible => {
                    ctx.enable_invisible();
                    invisible = true;

                    (false, None)
                }

                // If container is "terminating" (e.g. blockquote, p), then add newlines.
                // Also, determine if we add a prefix.
                ContainerType::Div | ContainerType::Paragraph => (true, None),
                ContainerType::Blockquote => (true, Some("    ")),
                ContainerType::Header(heading) => {
                    (true, Some(heading.level.prefix_with_space()))
                }

                // Wrap any ruby text with parentheses
                ContainerType::RubyText => {
                    ctx.push('(');

                    (false, None)
                }

                // Wrap form with [[form]] and [[/form]]
                ContainerType::Form => {
                    ctx.push_str("[[form]]");

                    (true, None)
                }

                // Inline or miscellaneous container.
                _ => (false, None),
            };

            if add_newlines {
                // Add prefix, if there's one
                if let Some(prefix) = prefix {
                    ctx.push_prefix(prefix);
                }

                ctx.add_newline();
            }

            // Render internal elements
            render_elements(ctx, container.elements());

            // Wrap any ruby text with parentheses
            match container.ctype() {
                ContainerType::RubyText => {
                    ctx.push(')');
                }

                ContainerType::Form => {
                    ctx.push_str("[[/form]]");
                }

                _ => {}
            }

            if add_newlines {
                // Pop prefix, if there's one
                if prefix.is_some() {
                    ctx.pop_prefix();
                }

                ctx.add_newline();
            }

            if invisible {
                ctx.disable_invisible();
            }
        }
        Element::Module(module) => {
            // for now: do nothing. text rendering is not supported on Python side yet
            ctx.push_str("[[module ");
            ctx.push_str(module.name());
            ctx.push_str("]]")
        }
        Element::Text(text) | Element::Raw(text) | Element::Email(text) => {
            ctx.push_str(text)
        }
        Element::HtmlEntity(text) => {
            ctx.push_str("<");
            ctx.push_str(text);
            ctx.push_str(">");
        }
        Element::Variable(name) => {
            let value = match ctx.variables().get(name) {
                Some(value) => str!(value),
                None => format!("{{${name}}}"),
            };

            info!(
                "Rendering variable (name '{}', value {})",
                name.as_ref(),
                value,
            );
            ctx.push_str(&value);
        }
        Element::Table(table) => {
            if !ctx.ends_with_newline() {
                ctx.add_newline();
            }

            for row in &table.rows {
                ctx.push_str("|| ");

                for (i, cell) in row.cells.iter().enumerate() {
                    render_elements(ctx, &cell.elements);

                    if i < row.cells.len() - 1 {
                        ctx.push_str(" || ");
                    }
                }

                ctx.push_str(" ||");
                ctx.add_newline();
            }

            ctx.add_newline();
        }
        Element::TabView(tabs) => {
            for Tab { label, elements } in tabs {
                // Add tab name
                str_write!(ctx, "[{label}]");
                ctx.add_newline();

                // Add tab contents
                render_elements(ctx, elements);
                ctx.add_newline();
            }
        }
        Element::Anchor {
            elements,
            attributes,
            ..
        } => {
            render_elements(ctx, elements);

            if let Some(href) = attributes.get().get("href") {
                let link = LinkLocation::parse(cow!(href), ctx.callbacks());
                let url = get_url_from_link(ctx, &link);

                str_write!(ctx, " [{url}]");
            }
        }
        Element::AnchorName(_) => {
            // Anchor names are an invisible addition to the HTML
            // to aid navigation. So in text mode, they are ignored.
        }
        Element::Link { link, label, .. } => {
            let url = get_url_from_link(ctx, link);

            let label = {
                let mut o_label: String = String::new();
                ctx.handle().get_link_label(link, label, |label| {
                    o_label = label.to_owned();
                });
                o_label
            };

            ctx.push_str(&label);
            if url != label && !url.starts_with('#') {
                str_write!(ctx, " [{url}]");
            }
        }
        Element::Image {
            source,
            link,
            attributes,
            ..
        } => {
            let source_url =
                ctx.handle()
                    .get_image_link(source, ctx.info(), ctx.settings());

            if let Some(url) = source_url {
                ctx.push_str(&url);

                if let Some(link) = link {
                    ctx.push(' ');
                    ctx.push_str(&get_url_from_link(ctx, link));
                }

                if let Some(alt_text) = attributes.get().get("alt") {
                    ctx.push(' ');
                    ctx.push_str(alt_text);
                }

                if let Some(title) = attributes.get().get("title") {
                    ctx.push(' ');
                    ctx.push_str(title);
                }
            }
        }
        Element::List { ltype, items, .. } => {
            if !ctx.ends_with_newline() {
                ctx.add_newline();
            }

            for item in items {
                match item {
                    ListItem::Elements { elements, hidden, .. } => {
                        // Don't do anything if it's empty
                        if elements.is_empty() {
                            continue;
                        }

                        // Render bullet and its depth
                        let depth = ctx.list_depth();
                        for _ in 0..depth {
                            ctx.push(' ');
                        }

                        if !*hidden {
                            match *ltype {
                                ListType::Bullet => ctx.push_str("* "),
                                ListType::Numbered => {
                                    let index = ctx.next_list_index();
                                    str_write!(ctx, "{index}. ");
                                }
                                ListType::Generic => (),
                            }
                        }

                        // Render elements for this list item
                        ctx.incr_list_depth();
                        render_elements(ctx, elements);
                        ctx.decr_list_depth();
                        ctx.add_newline();
                    }
                    ListItem::SubList { element } => {
                        // Update bullet depth
                        ctx.incr_list_depth();
                        render_element(ctx, element);
                        ctx.decr_list_depth();
                    }
                }
            }
        }
        Element::DefinitionList(items) => {
            for DefinitionListItem { key, value } in items {
                str_write!(ctx, ": ");
                render_elements(ctx, key);
                str_write!(ctx, " : ");
                render_elements(ctx, value);
                ctx.add_newline();
            }

            ctx.add_newline();
        }
        Element::Collapsible {
            elements,
            show_text,
            hide_text,
            show_top,
            show_bottom,
            ..
        } => {

            let show_text = match show_text {
                Some(s) => String::from(s.as_ref()),
                _ => ctx.handle().get_message("collapsible-open")
            };
        
            let hide_text = match hide_text {
                Some(s) => String::from(s.as_ref()),
                _ => ctx.handle().get_message("collapsible-hide")
            };

            // Top of collapsible
            ctx.add_newline();
            ctx.push_str(&show_text);
            ctx.add_newline();

            if *show_top {
                ctx.push_str(&hide_text);
                ctx.add_newline();
            }

            // Collapsible contents
            render_elements(ctx, elements);

            // Bottom of collapsible
            if *show_bottom {
                ctx.add_newline();
                ctx.push_str(&hide_text);
                ctx.add_newline();
            }
        }
        Element::FormInput(FormInput{ attributes }) => {
            info!("Rendering form input");

            ctx.push_str(&format!("[[input"));
            for (k, v) in attributes.get().iter() {
                ctx.push_str(&format!(" {k}=\"{v}\""));
            }
            ctx.push_str("]]");
            ctx.add_newline();
        }
        Element::TableOfContents { .. } => {
            info!("Rendering table of contents");

            let table_of_contents_title = ctx
                .handle()
                .get_message("table-of-contents");

            ctx.add_newline();
            ctx.push_str(&table_of_contents_title);
            ctx.add_newline();
            render_elements(ctx, ctx.table_of_contents());
        }
        Element::Footnote => {
            info!("Rendering footnote reference");

            let index = ctx.next_footnote_index();
            str_write!(ctx, "[{}]", index);
        }
        Element::FootnoteBlock { title, hide } => {
            info!("Rendering footnote block");

            if *hide || ctx.footnotes().is_empty() {
                return;
            }

            // Render footnote title
            let title_default;
            let title = match title {
                Some(title) => String::from(title.as_ref()),
                None => {
                    title_default = ctx
                        .handle()
                        .get_message("footnote-block-title");

                    title_default
                }
            };

            ctx.add_newline();
            ctx.push_str(&title);
            ctx.add_newline();

            // Render footnotes in order.
            for (index, contents) in ctx.footnotes().iter().enumerate() {
                str_write!(ctx, "{}. ", index + 1);

                render_elements(ctx, contents);
                ctx.add_newline();
            }
        }
        Element::User { name, .. } => ctx.push_str(name),
        Element::Date { value, .. } => {
            str_write!(ctx, "{}", value.format(Some((*value).default_format_string())));
        }
        Element::Color { elements, .. } => render_elements(ctx, elements),
        Element::Code { contents, language } => {
            let language = match language {
                Some(language) => language,
                None => "",
            };

            str_write!(ctx, "```{language}");
            ctx.add_newline();
            ctx.push_str(contents);
            ctx.add_newline();
            ctx.push_str("```");
        }
        Element::Math { name, latex_source } => {
            let index = ctx.next_equation_index();

            str_write!(ctx, "{index}.");
            if let Some(name) = name {
                str_write!(ctx, " ({name})");
            }

            ctx.add_newline();
            ctx.push_str("```latex");
            ctx.add_newline();
            ctx.push_str(latex_source);
            ctx.add_newline();
            ctx.push_str("```");
        }
        Element::MathInline { latex_source } => {
            str_write!(ctx, "[[$ {latex_source} $]]");
        }
        Element::EquationReference(name) => {
            str_write!(ctx, "[{name}]");
        }
        Element::Html { contents, external: _ } => {
            str_write!(ctx, "```html\n{contents}\n```");
        }
        Element::Iframe { url, .. } => str_write!(ctx, "[{url}]"),
        Element::Include {
            variables,
            elements,
            ..
        } => {
            info!(
                "Rendering include (variables length {}, elements length {})",
                variables.len(),
                elements.len(),
            );

            ctx.variables_mut().push_scope(variables);
            render_elements(ctx, elements);
            ctx.variables_mut().pop_scope();
        }
        Element::LineBreak => ctx.add_newline(),
        Element::LineBreaks(amount) => {
            for _ in 0..amount.get() {
                ctx.add_newline();
            }
        }
        Element::ClearFloat(_) => {
            if !ctx.ends_with_newline() {
                ctx.add_newline();
            }

            ctx.push_str("~~~~~~");
            ctx.add_newline();
        }
        Element::HorizontalRule => {
            if !ctx.ends_with_newline() {
                ctx.add_newline();
            }

            ctx.push_str("------");
            ctx.add_newline();
        }
        Element::Partial(_) => panic!("Encountered partial element during parsing"),
        Element::Void => {},
    }
}

fn get_url_from_link<'a>(_ctx: &TextContext, link: &'a LinkLocation<'a>) -> Cow<'a, str> {
    let url = normalize_link(link);

    // TODO: when we remove inline javascript stuff
    if url.as_ref() == "javascript:;" {
        return Cow::Borrowed("#");
    }

    url
}
