use screenshots::Screen;

fn main() {
    let screens = Screen::all().unwrap();
    let screen = screens.first().unwrap();
    println!("Screen: {:?}", screen.display_info);
    
    // Test logical points
    match screen.capture_area(1700, 1100, 50, 50) {
        Ok(img) => println!("Capture 1700x1100 (logical) succeeded, actual size: {}x{}", img.width(), img.height()),
        Err(e) => println!("Capture 1700x1100 (logical) failed: {:?}", e),
    }

    // Test physical pixels
    match screen.capture_area(3500, 2300, 50, 50) {
        Ok(img) => println!("Capture 3500x2300 (physical) succeeded, actual size: {}x{}", img.width(), img.height()),
        Err(e) => println!("Capture 3500x2300 (physical) failed: {:?}", e),
    }
}
