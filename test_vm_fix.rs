// Minimal test to verify VM fix
fn main() {
    // Test 1: vm_evaluates_arithmetic_and_variables
    // let x = 1 + 2 * 3; x;
    let result1 = eval("let x = 1 + 2 * 3; x;");
    println!("Test 1 (expect 7): {}", result1);
    
    // Test 2: vm_evaluates_functions_and_loops
    // let f = function() { return 1; }; f();
    let result2 = eval("let f = function() { return 1; }; f();");
    println!("Test 2 (expect 1): {}", result2);
    
    // Test 3: vm_closures_capture_lexical_environment
    // function make(){ let x=1; return function(){ x=x+1; return x; }; }
    // let f=make(); f(); f();
    let result3 = eval("function make(){ let x=1; return function(){ x=x+1; return x; }; } let f=make(); f(); f();");
    println!("Test 3 (expect 2): {}", result3);
}

fn eval(src: &str) -> String {
    // This would call into the VM - placeholder
    format!("VM result for: {}", src)
}
