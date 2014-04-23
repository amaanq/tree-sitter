#include "compiler/build_tables/parse_item.h"
#include "compiler/build_tables/get_metadata.h"
#include "tree_sitter/compiler.h"

namespace tree_sitter {
    using std::string;
    using std::to_string;
    using std::ostream;

    namespace build_tables {
        ParseItem::ParseItem(const rules::ISymbol &lhs,
                             const rules::rule_ptr rule,
                             size_t consumed_symbol_count,
                             const rules::ISymbol &lookahead_sym) :
            Item(lhs, rule),
            consumed_symbol_count(consumed_symbol_count),
            lookahead_sym(lookahead_sym) {}

        bool ParseItem::operator==(const ParseItem &other) const {
            bool lhs_eq = other.lhs == lhs;
            bool rules_eq = (*other.rule == *rule);
            bool consumed_sym_counts_eq = (other.consumed_symbol_count == consumed_symbol_count);
            bool lookaheads_eq = other.lookahead_sym == lookahead_sym;
            return lhs_eq && rules_eq && consumed_sym_counts_eq && lookaheads_eq;
        }

        int ParseItem::precedence() const {
            return get_metadata(rule, rules::PRECEDENCE);
        }

        ostream& operator<<(ostream &stream, const ParseItem &item) {
            return stream <<
            string("#<item ") <<
            item.lhs <<
            string(" ") <<
            *item.rule <<
            string(" ") <<
            to_string(item.consumed_symbol_count) <<
            string(" ") <<
            item.lookahead_sym <<
            string(">");
        }
    }
}

