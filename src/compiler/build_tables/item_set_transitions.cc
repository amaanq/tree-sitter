#include "compiler/build_tables/item_set_transitions.h"
#include <unordered_set>
#include "compiler/build_tables/item_set_closure.h"
#include "compiler/build_tables/rule_transitions.h"
#include "compiler/build_tables/merge_transitions.h"
#include "compiler/rules/interned_symbol.h"

namespace tree_sitter {
    using std::map;
    using std::unordered_set;
    using rules::CharacterSet;
    using rules::ISymbol;

    namespace build_tables {
        map<CharacterSet, LexItemSet>
        char_transitions(const LexItem &item) {
            map<CharacterSet, LexItemSet> result;
            for (auto &transition : char_transitions(item.rule)) {
                LexItem next_item(item.lhs, transition.second);
                result.insert({ transition.first, LexItemSet({ next_item }) });
            }
            return result;
        }

        map<ISymbol, ParseItemSet>
        sym_transitions(const ParseItem &item, const PreparedGrammar &grammar) {
            map<ISymbol, ParseItemSet> result;
            for (auto transition : sym_transitions(item.rule)) {
                ISymbol rule = transition.first;
                ParseItem new_item(item.lhs, transition.second, item.consumed_symbol_count + 1, item.lookahead_sym);
                result.insert({ rule, item_set_closure(ParseItemSet({ new_item }), grammar) });
            }
            return result;
        }

        template<typename T>
        static unordered_set<T> merge_sets(const unordered_set<T> &left, const unordered_set<T> &right) {
            unordered_set<T> result = left;
            result.insert(right.begin(), right.end());
            return result;
        }

        map<CharacterSet, LexItemSet>
        char_transitions(const LexItemSet &item_set, const PreparedGrammar &grammar) {
            map<CharacterSet, LexItemSet> result;
            for (const LexItem &item : item_set) {
                map<CharacterSet, LexItemSet> item_transitions = char_transitions(item);
                result = merge_char_transitions<LexItemSet>(result,
                                                            item_transitions,
                                                            [](LexItemSet left, LexItemSet right) {
                    return merge_sets(left, right);
                });
            }
            return result;
        }

        map<ISymbol, ParseItemSet>
        sym_transitions(const ParseItemSet &item_set, const PreparedGrammar &grammar) {
            map<ISymbol, ParseItemSet> result;
            for (const ParseItem &item : item_set) {
                map<ISymbol, ParseItemSet> item_transitions = sym_transitions(item, grammar);
                result = merge_sym_transitions<ParseItemSet>(result,
                                                             item_transitions,
                                                             [&](ParseItemSet left, ParseItemSet right) {
                    return merge_sets(left, right);
                });
            }
            return result;
        }
    }
}