//! Trace view rendering

use crate::tui::theme::Theme;
use crate::tui::trace::{TraceNode, TraceResult};
use crate::tui::views::props::{Fetched, TextScroll};
use crate::tui::views::text::{wrapped, wrapped_height};
use crate::tui::views::yaml::decorate_title_with_search;
use ratatui::{
    Frame,
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Widget},
};

/// Rows between blocks, for the arrow and its label.
const ARROW_HEIGHT: u16 = 3;

/// One block of the trace view.
#[derive(Debug, Clone)]
pub(crate) enum TraceBlock {
    /// An object on the path, or the source.
    Node(Box<TraceNode>),
    /// Where the trace stopped short of Flux (#266): what manages the chain
    /// instead, or why it ends.
    Stop { title: String, detail: String },
}

impl TraceBlock {
    /// Whether any of the block's displayed text matches the search query.
    fn matches(&self, query_lower: &str) -> bool {
        let haystacks: Vec<&str> = match self {
            TraceBlock::Node(node) => {
                let mut haystacks: Vec<&str> = vec![&node.kind, &node.name, &node.namespace];
                if let Some(ref spec) = node.spec {
                    haystacks.extend(spec.path.as_deref());
                    haystacks.extend(spec.url.as_deref());
                    haystacks.extend(spec.branch.as_deref());
                }
                if let Some(ref status) = node.status {
                    haystacks.extend(status.revision.as_deref());
                    haystacks.extend(status.message.as_deref());
                }
                haystacks
            }
            TraceBlock::Stop { title, detail } => vec![title, detail],
        };
        haystacks
            .iter()
            .any(|text| text.to_lowercase().contains(query_lower))
    }

    /// The block's text, before wrapping. `is_primary` marks the traced
    /// object itself.
    fn lines(&self, is_primary: bool, theme: &Theme) -> Vec<Line<'static>> {
        match self {
            TraceBlock::Node(node) => node_lines(node, is_primary, theme),
            TraceBlock::Stop { title, detail } => vec![
                Line::from(Span::styled(
                    title.clone(),
                    Style::default()
                        .fg(theme.text_primary)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(
                    detail.clone(),
                    Style::default()
                        .fg(theme.text_secondary)
                        .add_modifier(Modifier::ITALIC),
                )),
            ],
        }
    }

    /// Border style: the current search match stands out, the traced
    /// object is labelled, the rest recede.
    fn border(&self, is_primary: bool, is_current_match: bool, theme: &Theme) -> Style {
        if is_current_match {
            Style::default()
                .fg(theme.text_value)
                .add_modifier(Modifier::BOLD)
        } else if is_primary && matches!(self, TraceBlock::Node(_)) {
            Style::default().fg(theme.text_label)
        } else {
            Style::default().fg(theme.text_secondary)
        }
    }
}

/// Render the trace view. The blocks are laid out at their wrapped height
/// and drawn off screen at full size; only the viewport's rows are copied
/// onto the frame, so any trace scrolls to its end and a block scrolled
/// partly off the top shows its lower rows.
pub fn render_resource_trace(
    f: &mut Frame,
    area: Rect,
    trace: Fetched<'_, TraceResult>,
    text: TextScroll<'_>,
    theme: &Theme,
) {
    let Fetched {
        data: trace_result,
        loading: trace_loading,
    } = trace;
    let TextScroll {
        offset: scroll_offset,
        search,
    } = text;
    if trace_loading {
        crate::tui::views::helpers::render_loading_state(
            f,
            area,
            "Trace",
            "Tracing resource... Walking ownership chain...",
            theme,
        );
        return;
    }

    let Some(trace_result) = trace_result else {
        crate::tui::views::helpers::render_empty_state(
            f,
            area,
            "Trace",
            "No trace data available",
            "Select a resource and press 't' to trace",
            theme,
        );
        return;
    };

    let blocks = trace_blocks(trace_result);

    // Text search: match whole blocks (the trace view is block-based, not line-based)
    let query_lower = search.query.to_lowercase();
    let match_indexes: Vec<usize> = if query_lower.is_empty() {
        Vec::new()
    } else {
        blocks
            .iter()
            .enumerate()
            .filter(|(_, (block, _))| block.matches(&query_lower))
            .map(|(idx, _)| idx)
            .collect()
    };
    search.total_matches = match_indexes.len();
    if !match_indexes.is_empty() {
        search.current_match = search.current_match.min(match_indexes.len() - 1);
    }
    let current_match_block = match_indexes.get(search.current_match).copied();

    let mut title = "Trace".to_string();
    decorate_title_with_search(&mut title, search);
    let outer_block = crate::tui::views::helpers::create_themed_block(&title, theme);
    let inner_area = outer_block.inner(area);
    f.render_widget(outer_block, area);

    // Lay out: each block at its wrapped height (inside its borders), with
    // an arrow between consecutive blocks.
    let width = inner_area.width;
    let laid: Vec<(Vec<Line<'static>>, u16)> = blocks
        .iter()
        .enumerate()
        .map(|(i, (block, _))| {
            let lines = block.lines(i == 0, theme);
            let height = wrapped_height(&lines, width.saturating_sub(2)).saturating_add(2);
            (lines, height)
        })
        .collect();
    let mut tops = Vec::with_capacity(laid.len());
    let mut total: u16 = 0;
    for (i, (_, height)) in laid.iter().enumerate() {
        if i > 0 {
            total = total.saturating_add(ARROW_HEIGHT);
        }
        tops.push(total);
        total = total.saturating_add(*height);
    }

    // Jump the scroll offset to the start of the current match's block
    if search.pending_jump {
        search.pending_jump = false;
        if let Some(&top) = current_match_block.and_then(|i| tops.get(i)) {
            *scroll_offset = usize::from(top);
        }
    }

    let viewport = crate::tui::views::scroll::Viewport::text(
        scroll_offset,
        usize::from(total),
        usize::from(inner_area.height),
    );

    // Draw the whole trace off screen…
    let mut canvas = Buffer::empty(Rect::new(0, 0, width, total));
    for (i, ((block, relationship), (lines, height))) in blocks.iter().zip(laid).enumerate() {
        let top = tops[i];
        if i > 0 {
            let arrow = Rect::new(0, top - ARROW_HEIGHT, width, ARROW_HEIGHT);
            render_arrow(&mut canvas, arrow, relationship, theme);
        }
        let mut frame = Block::default()
            .borders(Borders::ALL)
            .border_style(block.border(i == 0, Some(i) == current_match_block, theme));
        if matches!(block, TraceBlock::Stop { .. }) {
            frame = frame.border_type(BorderType::Rounded);
        }
        wrapped(lines)
            .block(frame)
            .render(Rect::new(0, top, width, height), &mut canvas);
    }

    // …and copy the rows the viewport shows.
    let screen = f.buffer_mut();
    for (screen_row, row) in viewport.range().enumerate() {
        let (Ok(row), Ok(screen_row)) = (u16::try_from(row), u16::try_from(screen_row)) else {
            break;
        };
        for x in 0..width {
            if let (Some(cell), Some(target)) = (
                canvas.cell((x, row)),
                screen.cell_mut((inner_area.x + x, inner_area.y + screen_row)),
            ) {
                *target = cell.clone();
            }
        }
    }

    viewport.render_scrollbar(f, area, theme);
}

/// A trace node with every cluster-supplied string made safe to draw
/// (control characters and escapes stripped, newlines flattened, capped).
fn sanitized(node: &TraceNode) -> TraceNode {
    use crate::kube::kind_list::sanitize_cell as clean;
    let opt = |text: &Option<String>| text.as_deref().map(clean);
    TraceNode {
        kind: clean(&node.kind),
        api_version: node.api_version.clone(),
        name: clean(&node.name),
        namespace: clean(&node.namespace),
        status: node.status.as_ref().map(|s| crate::trace::TraceStatus {
            ready: s.ready,
            message: opt(&s.message),
            last_reconciled: opt(&s.last_reconciled),
            revision: opt(&s.revision),
        }),
        spec: node.spec.as_ref().map(|s| crate::trace::TraceSpec {
            path: opt(&s.path),
            url: opt(&s.url),
            branch: opt(&s.branch),
            source_ref: s.source_ref.clone(),
        }),
    }
}

/// The blocks a trace shows, top to bottom, with the label of the arrow
/// leading into each: the object, its controller owners ("owned by"), the
/// Flux objects managing it ("managed by"), the source ("sourced from"),
/// and — when the trace ended somewhere other than Flux — where it stopped.
/// Everything read from the cluster is sanitized here.
pub(crate) fn trace_blocks(result: &TraceResult) -> Vec<(TraceBlock, &'static str)> {
    use crate::kube::kind_list::sanitize_cell;
    use crate::trace::TraceOutcome;
    let path = result.path();
    let mut blocks: Vec<(TraceBlock, &'static str)> = path
        .iter()
        .enumerate()
        .map(|(i, node)| {
            let relationship = match i {
                0 => "Object",
                _ if node.is_flux() => "managed by",
                _ => "owned by",
            };
            (TraceBlock::Node(Box::new(sanitized(node))), relationship)
        })
        .collect();
    if let Some(source) = &result.source {
        blocks.push((
            TraceBlock::Node(Box::new(sanitized(source))),
            "sourced from",
        ));
    }
    let top = path
        .last()
        .map(|n| sanitize_cell(&format!("{}/{}", n.kind, n.name)))
        .unwrap_or_else(|| "the object".to_string());
    let stop = match &result.outcome {
        TraceOutcome::Flux => None,
        TraceOutcome::Other(manager) => Some((
            TraceBlock::Stop {
                title: sanitize_cell(&manager.describe()),
                detail: "Not managed by Flux — the trace stops here".to_string(),
            },
            "managed by",
        )),
        TraceOutcome::Unmanaged => Some((
            TraceBlock::Stop {
                title: "No manager".to_string(),
                detail: format!(
                    "Nothing manages {top}: no Flux, Argo CD, or Helm marks, and no owner above it"
                ),
            },
            "",
        )),
        TraceOutcome::OwnerUnreadable(owner) => Some((
            TraceBlock::Stop {
                title: sanitize_cell(owner),
                detail: "This owner couldn't be read (RBAC, or deleted) — the trace stops here"
                    .to_string(),
            },
            "owned by",
        )),
    };
    blocks.extend(stop);
    blocks
}

/// A trace node's lines: kind and name, then the namespace and whatever
/// spec and status it carries. Long values wrap (the block grows to fit).
fn node_lines(node: &TraceNode, is_primary: bool, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(Span::styled(
        format!("{}: {}", node.kind, node.name),
        Style::default()
            .fg(if is_primary {
                theme.text_label
            } else {
                theme.text_primary
            })
            .add_modifier(Modifier::BOLD),
    ))];
    let mut field = |label: &str, value: &str| {
        lines.push(Line::from(vec![
            Span::styled(format!("{label}: "), Style::default().fg(theme.text_label)),
            Span::styled(value.to_string(), Style::default().fg(theme.text_value)),
        ]));
    };
    // Cluster-scoped objects have no namespace.
    if !node.namespace.is_empty() {
        field("Namespace", &node.namespace);
    }
    if let Some(spec) = &node.spec {
        for (label, value) in [
            ("Path", &spec.path),
            ("URL", &spec.url),
            ("Branch", &spec.branch),
        ] {
            if let Some(value) = value {
                field(label, value);
            }
        }
    }
    if let Some(status) = &node.status {
        for (label, value) in [
            ("Revision", &status.revision),
            ("Last reconciled", &status.last_reconciled),
            ("Message", &status.message),
        ] {
            if let Some(value) = value {
                field(label, value);
            }
        }
    }
    lines
}

/// Draw an arrow between blocks: a vertical line down the middle, ending
/// in `▼` with the relationship beside it ("owned by", "managed by").
fn render_arrow(buf: &mut Buffer, area: Rect, relationship: &str, theme: &Theme) {
    if area.height == 0 {
        return;
    }
    let style = Style::default().fg(theme.text_label);
    let pad = " ".repeat(usize::from(area.width / 2));
    let mut lines: Vec<Line> = (1..area.height)
        .map(|_| Line::from(Span::styled(format!("{pad}│"), style)))
        .collect();
    // The label follows the head on the same line — padding the line to the
    // full width first (as this once did) pushed the label out of view.
    let mut head = vec![Span::styled(format!("{pad}▼"), style)];
    if area.height >= 2 && !relationship.is_empty() {
        head.push(Span::styled(
            format!("  {relationship}"),
            style.add_modifier(Modifier::ITALIC),
        ));
    }
    lines.push(Line::from(head));
    Paragraph::new(lines).render(area, buf);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::state::TextSearchState;
    use ratatui::{Terminal, backend::TestBackend};

    fn node(kind: &str, name: &str) -> TraceNode {
        TraceNode {
            kind: kind.into(),
            api_version: None,
            name: name.into(),
            namespace: "flux-system".into(),
            status: None,
            spec: None,
        }
    }

    #[test]
    fn trace_blocks_show_owners_managers_source_and_where_it_stopped() {
        use crate::kube::ownership::Manager;
        use crate::trace::TraceOutcome;
        let pod = TraceResult {
            object: node("Pod", "web-abc"),
            chain: vec![
                node("Pod", "web-abc"),
                node("ReplicaSet", "web-7b5"),
                node("Deployment", "web"),
                node("Kustomization", "apps"),
            ],
            source: Some(node("GitRepository", "flux-system")),
            outcome: TraceOutcome::Flux,
        };
        let labels: Vec<(String, &str)> = trace_blocks(&pod)
            .into_iter()
            .map(|(block, rel)| match block {
                TraceBlock::Node(n) => (n.kind, rel),
                TraceBlock::Stop { title, .. } => (title, rel),
            })
            .collect();
        assert_eq!(
            labels,
            [
                ("Pod".to_string(), "Object"),
                ("ReplicaSet".to_string(), "owned by"),
                ("Deployment".to_string(), "owned by"),
                ("Kustomization".to_string(), "managed by"),
                ("GitRepository".to_string(), "sourced from"),
            ]
        );
        assert_eq!(
            pod.flux_manager().map(|n| n.name.as_str()),
            Some("apps"),
            "g / m from the trace open the Kustomization"
        );

        let stop_of = |result: &TraceResult| match trace_blocks(result).pop() {
            Some((TraceBlock::Stop { title, detail }, rel)) => (title, detail, rel),
            other => panic!("expected a stop block, got {other:?}"),
        };

        // Argo CD tracking ids are free-form: sanitized before display.
        let argo = TraceResult {
            object: node("Deployment", "guestbook"),
            chain: vec![node("Deployment", "guestbook")],
            source: None,
            outcome: TraceOutcome::Other(Manager::ArgoCd {
                app: "guest\x1b[31mbook".into(),
            }),
        };
        let (title, detail, rel) = stop_of(&argo);
        assert_eq!(title, "Argo CD application guest[31mbook");
        assert!(detail.contains("stops here"));
        assert_eq!(rel, "managed by");
        assert!(argo.flux_manager().is_none());

        let bare = TraceResult {
            object: node("ConfigMap", "settings"),
            chain: vec![node("ConfigMap", "settings")],
            source: None,
            outcome: TraceOutcome::Unmanaged,
        };
        assert_eq!(trace_blocks(&bare).len(), 2);
        let (title, detail, rel) = stop_of(&bare);
        assert_eq!(title, "No manager");
        assert!(detail.contains("ConfigMap/settings"), "{detail}");
        assert_eq!(rel, "", "no 'managed by' leading to 'no manager'");

        let unreadable = TraceResult {
            object: node("Pod", "web-abc"),
            chain: vec![node("Pod", "web-abc")],
            source: None,
            outcome: TraceOutcome::OwnerUnreadable("ReplicaSet/web-7b5".into()),
        };
        let (title, detail, rel) = stop_of(&unreadable);
        assert_eq!((title.as_str(), rel), ("ReplicaSet/web-7b5", "owned by"));
        assert!(detail.contains("couldn't be read"));
    }

    #[test]
    fn a_kind_named_like_flux_from_another_group_is_not_flux() {
        let mut bucket = node("Bucket", "data");
        bucket.api_version = Some("s3.aws.upbound.io/v1beta1".into());
        assert!(!bucket.is_flux());
        bucket.api_version = Some("source.toolkit.fluxcd.io/v1".into());
        assert!(bucket.is_flux());
        bucket.api_version = None;
        assert!(bucket.is_flux(), "by name when the group is unknown");
    }

    /// Screen text of `result` drawn at `width`×`height`, scrolled to
    /// `offset` (clamped by the viewport, as in the app).
    fn screen(result: &TraceResult, width: u16, height: u16, offset: usize) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let mut offset = offset;
        terminal
            .draw(|f| {
                render_resource_trace(
                    f,
                    f.area(),
                    Fetched::new(Some(result), false),
                    TextScroll::new(&mut offset, &mut TextSearchState::default()),
                    &Theme::default(),
                )
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect())
            .collect()
    }

    fn long_chain() -> TraceResult {
        TraceResult {
            object: node("Pod", "web-abc"),
            chain: vec![
                node("Pod", "web-abc"),
                node("ReplicaSet", "web-7b5"),
                node("Deployment", "web"),
                node("Kustomization", "apps"),
            ],
            source: Some(node("GitRepository", "flux-system")),
            outcome: crate::trace::TraceOutcome::Flux,
        }
    }

    #[test]
    fn a_trace_taller_than_the_screen_scrolls_to_its_end() {
        let result = long_chain();
        let top = screen(&result, 60, 12, 0).join("\n");
        assert!(top.contains("Pod: web-abc"), "{top}");
        assert!(!top.contains("GitRepository"), "{top}");

        // Far past the end: the viewport clamps to the last screenful.
        let bottom = screen(&result, 60, 12, 1_000);
        let bottom_text = bottom.join("\n");
        assert!(
            bottom_text.contains("GitRepository: flux-system"),
            "{bottom_text}"
        );
        assert!(!bottom_text.contains("Pod: web-abc"), "{bottom_text}");
        // The last block's bottom border is the last content row.
        assert!(bottom[bottom.len() - 2].contains('└'), "{bottom_text}");
    }

    #[test]
    fn a_block_scrolled_partly_off_the_top_shows_its_lower_rows() {
        // One row in: the first block's top border is off screen, its
        // title is the first row shown.
        let rows = screen(&long_chain(), 60, 12, 1);
        assert!(rows[1].contains("Pod: web-abc"), "{rows:#?}");
        let rows = screen(&long_chain(), 60, 12, 2);
        assert!(rows[1].contains("Namespace: flux-system"), "{rows:#?}");
    }

    #[test]
    fn stop_blocks_grow_to_fit_their_wrapped_text() {
        let unreadable = TraceResult {
            object: node("Pod", "web-abc"),
            chain: vec![node("Pod", "web-abc")],
            source: None,
            outcome: crate::trace::TraceOutcome::OwnerUnreadable("ReplicaSet/web-7b5".into()),
        };
        // Narrow: the ~70-character detail wraps over several rows.
        let text = screen(&unreadable, 34, 30, 0).join("\n");
        assert!(text.contains("stops here"), "{text}");
        assert!(text.contains("couldn't be"), "{text}");
    }

    #[test]
    fn long_multibyte_messages_wrap_without_panicking() {
        let mut n = node("Kustomization", "apps");
        n.status = Some(crate::trace::TraceStatus {
            ready: Some(false),
            message: Some("é".repeat(200)),
            last_reconciled: None,
            revision: None,
        });
        let result = TraceResult {
            object: n,
            chain: vec![],
            source: None,
            outcome: crate::trace::TraceOutcome::Flux,
        };
        let mut terminal = Terminal::new(TestBackend::new(40, 20)).unwrap();
        terminal
            .draw(|f| {
                render_resource_trace(
                    f,
                    f.area(),
                    Fetched::new(Some(&result), false),
                    TextScroll::new(&mut 0, &mut TextSearchState::default()),
                    &Theme::default(),
                )
            })
            .unwrap();
    }

    #[test]
    fn trace_shows_every_flux_manager_in_the_chain() {
        let result = TraceResult {
            object: node("Deployment", "source-controller"),
            chain: vec![
                node("Deployment", "source-controller"),
                node("FluxInstance", "flux"),
            ],
            source: None,
            outcome: crate::trace::TraceOutcome::Flux,
        };
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
        terminal
            .draw(|f| {
                render_resource_trace(
                    f,
                    f.area(),
                    Fetched::new(Some(&result), false),
                    TextScroll::new(&mut 0, &mut TextSearchState::default()),
                    &Theme::default(),
                )
            })
            .unwrap();
        let out: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(out.contains("FluxInstance: flux"), "{out}");
        assert!(
            out.contains("managed by"),
            "arrow labels are visible: {out}"
        );
    }
}
