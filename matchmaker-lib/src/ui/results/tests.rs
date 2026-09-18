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
fn test_parse_template_color_tags() {
    let input = "{yellow}󰕌 Undone copy of 1 item{reset}";
    let line = StatusUI::parse_template_to_status_line(input);
    assert_eq!(line.spans.len(), 1);
    assert_eq!(line.spans[0].content, "󰕌 Undone copy of 1 item");
    assert_eq!(line.spans[0].style.fg, Some(Color::Yellow));

    let input_mixed = "Copied: {yellow}3 items{reset} into {cyan}folder{reset}";
    let line_mixed = StatusUI::parse_template_to_status_line(input_mixed);
    assert_eq!(line_mixed.spans.len(), 4);
    assert_eq!(line_mixed.spans[0].content, "Copied: ");
    assert_eq!(line_mixed.spans[0].style.fg, None);
    assert_eq!(line_mixed.spans[1].content, "3 items");
    assert_eq!(line_mixed.spans[1].style.fg, Some(Color::Yellow));
    assert_eq!(line_mixed.spans[2].content, " into ");
    assert_eq!(line_mixed.spans[2].style.fg, None);
    assert_eq!(line_mixed.spans[3].content, "folder");
    assert_eq!(line_mixed.spans[3].style.fg, Some(Color::Cyan));
}

#[test]
fn test_parse_template_colon_style() {
    let input = "{yellow:󰕌 Undone copy of 1 item}";
    let line = StatusUI::parse_template_to_status_line(input);
    assert_eq!(line.spans.len(), 1);
    assert_eq!(line.spans[0].content, "󰕌 Undone copy of 1 item");
    assert_eq!(line.spans[0].style.fg, Some(Color::Yellow));
}

#[test]
fn test_results_ui_renders_tier_separator() {
    let mut results_config = ResultsConfig::default();
    results_config.tier.separator = HorizontalSeparator::Top;
    results_config.tier.style = StyleSetting {
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
    results_config.tier.separator = HorizontalSeparator::Underline;
    results_config.tier.style = StyleSetting {
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

#[test]
fn test_results_ui_bookmark_styling_focused_and_unfocused() {
    let bookmark_color = Color::Rgb(249, 226, 175);
    let mut results_config = ResultsConfig::default();
    results_config.icons = true;
    results_config.uncolor_current_icon = true;
    results_config.bookmark.icon_style = StyleSetting {
        fg: Some(bookmark_color),
        ..Default::default()
    };
    results_config.current_style = StyleSetting {
        fg: Some(Color::Black),
        bg: Some(Color::DarkGray),
        modifier: Modifier::BOLD,
    };

    let status_config = StatusConfig::default();
    let mut results_ui = ResultsUI::new(results_config, status_config);
    let area = ratatui::layout::Rect::new(0, 0, 40, 10);
    results_ui.update_dimensions(&area);
    results_ui.pin_paths.insert("bookmarked.txt".to_string());

    let mut worker = Worker::<String>::new_single_column();
    let injector = worker.nucleo.injector();
    injector.push("bookmarked.txt".to_string(), |item, cols| {
        cols[0] = item.clone().into()
    });
    injector.push("regular.txt".to_string(), |item, cols| {
        cols[0] = item.clone().into()
    });
    worker.nucleo.tick(10);

    let mut selector = crate::selector::Selector::new(|_s: &String| (0u32, ())).disabled();
    let mut matcher = nucleo::Matcher::default();
    let mut click = Click::None;

    use ratatui::widgets::Widget;
    let render_area = ratatui::layout::Rect::new(0, 0, 40, 5);

    // 1. When cursor is at 1: bookmarked.txt (row 1) is focused, regular.txt (row 0) is unfocused
    results_ui.cursor = 1;
    let table = results_ui.make_table(
        0,
        &mut worker,
        &mut selector,
        &mut matcher,
        &mut click,
        None,
        false,
    );
    let mut buf = ratatui::buffer::Buffer::empty(render_area);
    table.render(render_area, &mut buf);

    // Row 0 has "regular.txt", Row 1 has "bookmarked.txt"
    let row0_text: String = (0..40).map(|x| buf[(x, 0)].symbol()).collect();
    let row1_text: String = (0..40).map(|x| buf[(x, 1)].symbol()).collect();
    assert!(row0_text.contains("regular.txt"));
    assert!(row1_text.contains("bookmarked.txt"));

    // Row 1 (bookmarked.txt, focused):
    // It should have foreground = bookmark_color (NOT black!), background = DarkGray, modifier = BOLD!
    let b_x = (0..40).find(|&x| buf[(x, 1)].symbol() == "b").unwrap();
    let cell_b_focused = &buf[(b_x, 1)];
    assert_eq!(cell_b_focused.fg, bookmark_color);
    assert_eq!(cell_b_focused.bg, Color::DarkGray);
    assert!(cell_b_focused.modifier.contains(Modifier::BOLD));

    // Also verify the bookmark icon glyph has foreground = bookmark_color even on focus!
    let icon_x = (0..40).find(|&x| buf[(x, 1)].symbol() == "󱀻").unwrap();
    let icon_cell = &buf[(icon_x, 1)];
    assert_eq!(icon_cell.fg, bookmark_color);
    assert_eq!(icon_cell.bg, Color::DarkGray);

    // Row 0 (regular.txt, unfocused):
    let r_x = (0..40).find(|&x| buf[(x, 0)].symbol() == "r").unwrap();
    let cell_r_unfocused = &buf[(r_x, 0)];
    assert_ne!(cell_r_unfocused.fg, bookmark_color);
    assert_ne!(cell_r_unfocused.bg, Color::DarkGray);

    // 2. When cursor is at 0: regular.txt (row 0) is focused, bookmarked.txt (row 1) is unfocused
    results_ui.cursor = 0;
    let table = results_ui.make_table(
        0,
        &mut worker,
        &mut selector,
        &mut matcher,
        &mut click,
        None,
        false,
    );
    let mut buf = ratatui::buffer::Buffer::empty(render_area);
    table.render(render_area, &mut buf);

    // Row 1 (bookmarked.txt, unfocused):
    // Should still have foreground = bookmark_color!
    let b_x = (0..40).find(|&x| buf[(x, 1)].symbol() == "b").unwrap();
    let cell_b_unfocused = &buf[(b_x, 1)];
    assert_eq!(cell_b_unfocused.fg, bookmark_color);
    assert_ne!(cell_b_unfocused.bg, Color::DarkGray);

    // Row 0 (regular.txt, focused):
    // Foreground should be black, background DarkGray, modifier BOLD
    let r_x = (0..40).find(|&x| buf[(x, 0)].symbol() == "r").unwrap();
    let cell_r_focused = &buf[(r_x, 0)];
    assert_eq!(cell_r_focused.fg, Color::Black);
    assert_eq!(cell_r_focused.bg, Color::DarkGray);
    assert!(cell_r_focused.modifier.contains(Modifier::BOLD));
}

#[test]
fn test_results_ui_mode_2_bookmarks_and_folders_styling() {
    let bookmark_color = Color::Rgb(249, 226, 175);
    let mut results_config = ResultsConfig::default();
    results_config.icons = true;
    results_config.uncolor_current_icon = true;
    results_config.bookmark.icon_style = StyleSetting {
        fg: Some(bookmark_color),
        ..Default::default()
    };
    results_config.current_style = StyleSetting {
        fg: Some(Color::Black),
        bg: Some(Color::DarkGray),
        modifier: Modifier::BOLD,
    };

    let status_config = StatusConfig::default();
    let mut results_ui = ResultsUI::new(results_config, status_config);
    let area = ratatui::layout::Rect::new(0, 0, 40, 10);
    results_ui.update_dimensions(&area);
    results_ui.set_mode_index(2); // bookmarks mode

    let mut worker = Worker::<String>::new_single_column();
    worker.dir_first = true;
    let injector = worker.nucleo.injector();
    injector.push("my_folder/".to_string(), |item, cols| {
        cols[0] = item.clone().into()
    });
    injector.push("my_file.rs".to_string(), |item, cols| {
        cols[0] = item.clone().into()
    });
    worker.nucleo.tick(10);

    let mut selector = crate::selector::Selector::new(|_s: &String| (0u32, ())).disabled();
    let mut matcher = nucleo::Matcher::default();
    let mut click = Click::None;

    use ratatui::widgets::Widget;
    let render_area = ratatui::layout::Rect::new(0, 0, 40, 5);

    // Focus on my_folder/ (cursor = 0)
    results_ui.cursor = 0;
    let table = results_ui.make_table(
        0,
        &mut worker,
        &mut selector,
        &mut matcher,
        &mut click,
        None,
        false,
    );
    let mut buf = ratatui::buffer::Buffer::empty(render_area);
    table.render(render_area, &mut buf);

    // Folder on row 0:
    // Icon glyph is folder bookmark icon: "󰮟"
    let icon_folder_x = (0..40).find(|&x| buf[(x, 0)].symbol() == "󰮟").unwrap();
    let icon_folder_cell = &buf[(icon_folder_x, 0)];
    assert_eq!(icon_folder_cell.fg, bookmark_color);
    assert_eq!(icon_folder_cell.bg, Color::DarkGray);

    // Folder text on row 0: yellow fg, DarkGray bg, BOLD
    let f_x = (0..40).find(|&x| buf[(x, 0)].symbol() == "m").unwrap();
    let cell_f = &buf[(f_x, 0)];
    assert_eq!(cell_f.fg, bookmark_color);
    assert_eq!(cell_f.bg, Color::DarkGray);
    assert!(cell_f.modifier.contains(Modifier::BOLD));

    // File on row 2 (unfocused):
    // Icon glyph is file bookmark icon: "󱀻"
    let icon_file_x = (0..40).find(|&x| buf[(x, 2)].symbol() == "󱀻").unwrap();
    let icon_file_cell = &buf[(icon_file_x, 2)];
    assert_eq!(icon_file_cell.fg, bookmark_color);
    assert_ne!(icon_file_cell.bg, Color::DarkGray);

    // File text on row 2: yellow fg, transparent bg
    let file_m_x = (0..40).find(|&x| buf[(x, 2)].symbol() == "m").unwrap();
    let cell_file = &buf[(file_m_x, 2)];
    assert_eq!(cell_file.fg, bookmark_color);
    assert_ne!(cell_file.bg, Color::DarkGray);
}

#[test]
fn test_results_ui_cut_and_yanked_navbar_and_flash_icons() {
    let mut results_config = ResultsConfig::default();
    results_config.icons = true;
    results_config.current_nav_bar_style = StyleSetting {
        fg: Some(Color::Green),
        bg: Some(Color::DarkGray),
        ..Default::default()
    };
    results_config.current_style = StyleSetting {
        bg: Some(Color::DarkGray),
        ..Default::default()
    };

    let status_config = StatusConfig::default();
    let mut results_ui = ResultsUI::new(results_config, status_config);
    let area = ratatui::layout::Rect::new(0, 0, 40, 10);
    results_ui.update_dimensions(&area);

    let mut worker = Worker::<String>::new_single_column();
    worker.dir_first = true;
    let injector = worker.nucleo.injector();
    injector.push("cut_folder/".to_string(), |item, cols| {
        cols[0] = item.clone().into()
    });
    injector.push("yank_file.txt".to_string(), |item, cols| {
        cols[0] = item.clone().into()
    });
    worker.nucleo.tick(10);

    let mut selector = crate::selector::Selector::new(|_s: &String| (0u32, ())).disabled();
    let mut matcher = nucleo::Matcher::default();
    let mut click = Click::None;

    results_ui.cut_paths.insert("cut_folder/".to_string());
    results_ui.yank_paths.insert("yank_file.txt".to_string());
    results_ui.set_flash_target("cut_folder/".to_string(), FlashOp::Cut);
    results_ui.set_flash_target("yank_file.txt".to_string(), FlashOp::Copy);

    let nav_bar_style = Some((
        ratatui::widgets::BorderType::Thick,
        ratatui::style::Style::default().fg(Color::Blue),
    ));

    // Focus row 0 (cut_folder/)
    results_ui.cursor = 0;
    let table = results_ui.make_table(
        0,
        &mut worker,
        &mut selector,
        &mut matcher,
        &mut click,
        nav_bar_style,
        false,
    );

    use ratatui::widgets::Widget;
    let render_area = ratatui::layout::Rect::new(0, 0, 40, 5);
    let mut buf = ratatui::buffer::Buffer::empty(render_area);
    table.render(render_area, &mut buf);

    // Row 0 (cut_folder/) - Cut navbar is Red (even with current_nav_bar_style.fg = Green)
    let row0_nav_cell = &buf[(0, 0)];
    assert_eq!(row0_nav_cell.fg, Color::Red);
    assert_eq!(row0_nav_cell.bg, Color::DarkGray);

    // Row 0 Icon is Cut flash glyph: "󰆐" and Red
    let cut_icon_x = (0..40).find(|&x| buf[(x, 0)].symbol() == "󰆐").unwrap();
    let cut_icon_cell = &buf[(cut_icon_x, 0)];
    assert_eq!(cut_icon_cell.fg, Color::Red);

    // Row 2 (yank_file.txt) - Yanked navbar is Yellow
    let row2_nav_cell = &buf[(0, 2)];
    assert_eq!(row2_nav_cell.fg, Color::Yellow);

    // Row 2 Icon is Copy flash glyph: "󰆏" and Yellow
    let copy_icon_x = (0..40).find(|&x| buf[(x, 2)].symbol() == "󰆏").unwrap();
    let copy_icon_cell = &buf[(copy_icon_x, 2)];
    assert_eq!(copy_icon_cell.fg, Color::Yellow);

    // --- After flash expiry (e.g. > 1.5s): flash_targets is cleared, but items remain cut/yanked ---
    results_ui.flash_targets.clear();
    let table_post_flash = results_ui.make_table(
        0,
        &mut worker,
        &mut selector,
        &mut matcher,
        &mut click,
        nav_bar_style,
        false,
    );
    let mut buf_post_flash = ratatui::buffer::Buffer::empty(render_area);
    table_post_flash.render(render_area, &mut buf_post_flash);

    // Row 0 (cut_folder/) - Navbar REMAINS Red
    let row0_post_nav = &buf_post_flash[(0, 0)];
    assert_eq!(row0_post_nav.fg, Color::Red);
    assert_eq!(row0_post_nav.bg, Color::DarkGray);

    // Row 0 Icon returned to folder glyph "\u{f115}" (nf-fa-folder_open) BUT color REMAINS Red
    let row0_icon_x = (0..40)
        .find(|&x| buf_post_flash[(x, 0)].symbol() == "\u{f115}")
        .unwrap();
    let row0_icon_cell = &buf_post_flash[(row0_icon_x, 0)];
    assert_eq!(row0_icon_cell.fg, Color::Red);

    // Row 2 (yank_file.txt) - Navbar REMAINS Yellow
    let row2_post_nav = &buf_post_flash[(0, 2)];
    assert_eq!(row2_post_nav.fg, Color::Yellow);

    // Row 2 Icon returned to text file glyph "\u{f15c}" BUT color REMAINS Yellow
    let row2_icon_x = (0..40)
        .find(|&x| buf_post_flash[(x, 2)].symbol() == "\u{f15c}")
        .unwrap();
    let row2_icon_cell = &buf_post_flash[(row2_icon_x, 2)];
    assert_eq!(row2_icon_cell.fg, Color::Yellow);
}
