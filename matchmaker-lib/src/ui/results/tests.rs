use super::*;
use crate::{
    config::{HorizontalSeparator, ResultsConfig, StatusConfig, StyleSetting},
    nucleo::Worker,
    render::Click,
};
use ratatui::style::{Color, Modifier};

#[test]
fn test_parse_template_to_status_line() {
    let input = " {cyan,bold:[Enter]} {darkgray:Switch Window}  •  {yellow,bold:[Space]} {dark_gray:Select}  •  {red,bold:[d]} {darkgray:Kill Window}  •  {blue,bold:[Esc]} {darkgray:Cancel}";
    let line = StatusUI::parse_template_to_status_line(input);
    println!("Spans: {:?}", line.spans);
    assert_eq!(line.spans[1].style.fg, Some(Color::Cyan));
    assert!(line.spans[1].style.add_modifier.contains(Modifier::BOLD));
    assert_eq!(line.spans[3].style.fg, Some(Color::DarkGray));
}

#[test]
fn test_results_ui_renders_tier_separator() {
    let mut results_config = ResultsConfig::default();
    results_config.tier_separator = HorizontalSeparator::Top;
    results_config.tier_separator_style = StyleSetting {
        fg: Some(Color::DarkGray),
        ..Default::default()
    };

    let status_config = StatusConfig::default();
    let mut results_ui = ResultsUI::new(results_config, status_config);
    let area = ratatui::layout::Rect::new(0, 0, 30, 10);
    results_ui.update_dimensions(&area);

    let mut worker = Worker::<String>::new_single_column();
    worker.dir_first = true;
    let injector = worker.nucleo.injector();
    injector.push("alpha/".to_string(), |item, cols| {
        cols[0] = item.clone().into()
    });
    injector.push("file.txt".to_string(), |item, cols| {
        cols[0] = item.clone().into()
    });
    worker.nucleo.tick(10);

    let mut selector = crate::selector::Selector::new(|_s: &String| (0u32, ())).disabled();
    let mut matcher = nucleo::Matcher::default();
    let mut click = Click::None;

    let table = results_ui.make_table(
        0,
        &mut worker,
        &mut selector,
        &mut matcher,
        &mut click,
        None,
        false,
    );
    let render_area = ratatui::layout::Rect::new(0, 0, 30, 5);
    let mut buf = ratatui::buffer::Buffer::empty(render_area);
    use ratatui::widgets::Widget;
    table.render(render_area, &mut buf);

    // Row 0 has "alpha/"
    let row0_text: String = (0..30).map(|x| buf[(x, 0)].symbol()).collect();
    assert!(row0_text.contains("alpha/"));

    // Row 1 is the tier separator line ("▔▔▔...")
    let row1_text: String = (0..30).map(|x| buf[(x, 1)].symbol()).collect();
    assert!(row1_text.contains("▔▔▔"));

    // Row 2 has "file.txt"
    let row2_text: String = (0..30).map(|x| buf[(x, 2)].symbol()).collect();
    assert!(row2_text.contains("file.txt"));
}

#[test]
fn test_results_ui_renders_tier_separator_underline() {
    let mut results_config = ResultsConfig::default();
    results_config.icons = true;
    results_config.tier_separator = HorizontalSeparator::Underline;
    results_config.tier_separator_style = StyleSetting {
        fg: Some(Color::Cyan),
        ..Default::default()
    };

    let status_config = StatusConfig::default();
    let mut results_ui = ResultsUI::new(results_config, status_config);
    let area = ratatui::layout::Rect::new(0, 0, 30, 10);
    results_ui.update_dimensions(&area);

    let mut worker = Worker::<String>::new_single_column();
    worker.dir_first = true;
    let injector = worker.nucleo.injector();
    injector.push("alpha/".to_string(), |item, cols| {
        cols[0] = item.clone().into()
    });
    injector.push("file.txt".to_string(), |item, cols| {
        cols[0] = item.clone().into()
    });
    worker.nucleo.tick(10);

    let mut selector = crate::selector::Selector::new(|_s: &String| (0u32, ())).disabled();
    let mut matcher = nucleo::Matcher::default();
    let mut click = Click::None;

    let table = results_ui.make_table(
        0,
        &mut worker,
        &mut selector,
        &mut matcher,
        &mut click,
        None,
        false,
    );
    let render_area = ratatui::layout::Rect::new(0, 0, 30, 5);
    let mut buf = ratatui::buffer::Buffer::empty(render_area);
    use ratatui::widgets::Widget;
    table.render(render_area, &mut buf);

    // Row 0 has "alpha/":
    // 1. Prefix gutter is NOT underlined
    let prefix_cell = &buf[(0, 0)];
    assert!(!prefix_cell.modifier.contains(Modifier::UNDERLINED));

    // 2. Icon cell is underlined
    let icon_cell = &buf[(3, 0)];
    assert!(icon_cell.modifier.contains(Modifier::UNDERLINED));

    // 3. Text cell is underlined
    let text_cell = &buf[(5, 0)];
    assert!(text_cell.modifier.contains(Modifier::UNDERLINED));

    // 4. Trailing padding cell is underlined across the entire row width
    let pad_cell = &buf[(25, 0)];
    assert!(pad_cell.modifier.contains(Modifier::UNDERLINED));

    // Row 1 has "file.txt" (no extra separator row!)
    let row1_text: String = (0..30).map(|x| buf[(x, 1)].symbol()).collect();
    assert!(row1_text.contains("file.txt"));
}
