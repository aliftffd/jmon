use super::{
    theme,
    widgets::{line, row},
};
use crate::model::Series;
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    symbols::Marker,
    text::{Line, Span},
    widgets::{Axis, Block, Chart, Dataset, GraphType},
    Frame,
};

pub fn history(
    f: &mut Frame,
    area: Rect,
    series: &Series,
    title: &str,
    ceiling: f64,
    color: Color,
    available: bool,
) {
    histories(f, area, &[(series, color)], title, ceiling, available);
}

/// Overlay histories on one scale; non-finite samples leave gaps in each trace.
pub fn histories(
    f: &mut Frame,
    area: Rect,
    series: &[(&Series, Color)],
    title: &str,
    ceiling: f64,
    available: bool,
) {
    if area.width < 12 || area.height < 5 {
        return;
    }
    let n = series.iter().map(|(s, _)| s.data.len()).max().unwrap_or(0);
    if !available || n == 0 {
        line(
            f,
            row(area, 0),
            Line::styled(format!("{title} / history n/a"), theme::muted()),
        );
        return;
    }
    // Split non-finite samples so a missing-sample sentinel never joins a gap.
    let mut segments = Vec::new();
    let mut floor = 0.0_f64;
    for (series, color) in series {
        let mut points = Vec::new();
        for (i, value) in series.data.iter().enumerate() {
            if value.is_finite() {
                floor = floor.min(*value);
                points.push(((n - series.data.len() + i) as f64, *value));
            } else if !points.is_empty() {
                segments.push((std::mem::take(&mut points), *color));
            }
        }
        if !points.is_empty() {
            segments.push((points, *color));
        }
    }
    let datasets = segments
        .iter()
        .map(|(points, color)| {
            Dataset::default()
                .data(points)
                .marker(Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(*color))
        })
        .collect::<Vec<_>>();
    let ceiling = if ceiling.is_finite() {
        ceiling.max(1.0)
    } else {
        1.0
    };
    let y_labels = if area.height >= 8 {
        vec![
            Span::raw(axis_label(floor)),
            Span::raw(axis_label((ceiling + floor) / 2.0)),
            Span::raw(axis_label(ceiling)),
        ]
    } else {
        vec![Span::raw(axis_label(floor)), Span::raw(axis_label(ceiling))]
    };
    let y_labels = y_labels
        .into_iter()
        .map(|s| s.style(theme::muted()))
        .collect::<Vec<_>>();
    let caption = format!("{title} / last {n} samples");
    let caption = if Line::from(caption.as_str()).width() <= area.width as usize {
        caption
    } else {
        title.to_string()
    };
    let chart = Chart::new(datasets)
        .style(theme::panel())
        .block(
            Block::default()
                .style(theme::panel())
                .title(Line::styled(caption, theme::muted())),
        )
        .x_axis(
            Axis::default()
                .style(theme::border())
                .bounds([0.0, n.saturating_sub(1).max(1) as f64])
                .labels([
                    Span::styled(format!("-{}", n.saturating_sub(1)), theme::muted()),
                    Span::styled("now", theme::muted()),
                ]),
        )
        .y_axis(
            Axis::default()
                .style(theme::border())
                .bounds([floor, ceiling])
                .labels(y_labels),
        );
    f.render_widget(chart, area);
}

fn axis_label(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    }
}
