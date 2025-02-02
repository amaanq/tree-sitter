const testing = @import("std").testing;

pub extern fn tree_sitter_PARSER_NAME() callconv(.C) *const anyopaque;

pub export fn language() *const anyopaque {
    return tree_sitter_PARSER_NAME();
}

test "can load grammar" {
    _ = language();
}
