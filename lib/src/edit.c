#include "./point.h"
#include "tree_sitter/api.h"

void ts_point_edit(TSPoint *point, uint32_t point_byte, const TSInputEdit *edit, uint32_t *new_byte) {
  uint32_t start_byte = point_byte;
  TSPoint start_point = *point;

  if (start_byte >= edit->old_end_byte) {
    start_byte = edit->new_end_byte + (start_byte - edit->old_end_byte);
    start_point = point_add(edit->new_end_point, point_sub(start_point, edit->old_end_point));
  } else if (start_byte > edit->start_byte) {
    start_byte = edit->new_end_byte;
    start_point = edit->new_end_point;
  }

  *point = start_point;
  if (new_byte) {
    *new_byte = start_byte;
  }
}

void ts_range_edit(TSRange *range, const TSInputEdit *edit) {
  // Edit the end position first
  if (range->end_byte >= edit->old_end_byte) {
    if (range->end_byte != UINT32_MAX) {
      range->end_byte = edit->new_end_byte + (range->end_byte - edit->old_end_byte);
      range->end_point = point_add(
        edit->new_end_point,
        point_sub(range->end_point, edit->old_end_point)
      );
      if (range->end_byte < edit->new_end_byte) {
        range->end_byte = UINT32_MAX;
        range->end_point = POINT_MAX;
      }
    }
  } else if (range->end_byte > edit->start_byte) {
    range->end_byte = edit->start_byte;
    range->end_point = edit->start_point;
  }

  // Edit the start position
  if (range->start_byte >= edit->old_end_byte) {
    range->start_byte = edit->new_end_byte + (range->start_byte - edit->old_end_byte);
    range->start_point = point_add(
      edit->new_end_point,
      point_sub(range->start_point, edit->old_end_point)
    );
    if (range->start_byte < edit->new_end_byte) {
      range->start_byte = UINT32_MAX;
      range->start_point = POINT_MAX;
    }
  } else if (range->start_byte > edit->start_byte) {
    range->start_byte = edit->start_byte;
    range->start_point = edit->start_point;
  }
}

void ts_ranges_edit(TSRange *ranges, uint32_t count, const TSInputEdit *edit) {
  for (unsigned i = 0; i < count; i++) {
    ts_range_edit(&ranges[i], edit);
  }
}
