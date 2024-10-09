#ifndef TREE_SITTER_LEXER_H_
#define TREE_SITTER_LEXER_H_

#ifdef __cplusplus
extern "C" {
#endif

#include "./length.h"
#include "tree_sitter/api.h"
#include "./parser.h"

typedef struct {
  uint32_t value: 31;
  bool valid: 1;
} ColumnData;

static const ColumnData COLUMN_NONE = {0, false};

static inline ColumnData column_add(ColumnData col1, ColumnData col2) {
  if (col1.valid && col2.valid) {
    return (ColumnData){.value = col1.value + col2.value, .valid = true};
  } else {
    return COLUMN_NONE;
  }
}

static inline ColumnData column_sub(ColumnData col1, ColumnData col2) {
  if (col1.valid && col2.valid) {
    return (ColumnData){.value = col1.value - col2.value, .valid = true};
  } else {
    return COLUMN_NONE;
  }
}

typedef struct {
  TSLexer data;
  Length current_position;
  Length token_start_position;
  ColumnData token_start_column;
  Length token_end_position;
  ColumnData token_end_column;

  TSRange *included_ranges;
  const char *chunk;
  TSInput input;
  TSLogger logger;

  uint32_t included_range_count;
  uint32_t current_included_range_index;
  uint32_t chunk_start;
  uint32_t chunk_size;
  uint32_t lookahead_size;
  bool did_get_column;
  ColumnData column_data;

  char debug_buffer[TREE_SITTER_SERIALIZATION_BUFFER_SIZE];
} Lexer;

void ts_lexer_init(Lexer *self);
void ts_lexer_delete(Lexer *self);
void ts_lexer_set_input(Lexer *self, TSInput input);
void ts_lexer_reset(Lexer *self, Length position, ColumnData column);
void ts_lexer_start(Lexer *self);
void ts_lexer_finish(Lexer *self, uint32_t *lookahead_end_byte);
void ts_lexer_advance_to_end(Lexer *self);
void ts_lexer_mark_end(Lexer *self);
bool ts_lexer_set_included_ranges(Lexer *self, const TSRange *ranges, uint32_t count);
TSRange *ts_lexer_included_ranges(const Lexer *self, uint32_t *count);

#ifdef __cplusplus
}
#endif

#endif  // TREE_SITTER_LEXER_H_
