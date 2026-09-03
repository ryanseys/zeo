require 'racc/parser.rb'
class TinyCalc < Racc::Parser

module_eval(<<'...end calc.y/module_eval...', 'calc.y', 12)
  def parse(tokens)
    @tokens = tokens
    do_parse
  end
  def next_token
    @tokens.shift
  end
...end calc.y/module_eval...
##### State transition tables begin ###

racc_action_table = [
     7,     8,     4,     5,     6,    15,     7,     8,     4,     5,
     4,     5,     4,     5,     4,     5,     9,    11,     9,     9 ]

racc_action_check = [
    10,    10,     0,     0,     1,    10,     1,     1,     5,     5,
     7,     7,     8,     8,     9,     9,     2,     6,    12,    13 ]

racc_action_pointer = [
    -3,     4,    12,   nil,   nil,     3,    17,     5,     7,     9,
    -2,   nil,    14,    15,   nil,   nil ]

racc_action_default = [
    -8,    -8,    -3,    -5,    -6,    -8,    -8,    -8,    -8,    -8,
    -8,    16,    -1,    -2,    -4,    -7 ]

racc_goto_table = [
     1,    12,    13,    14,   nil,    10 ]

racc_goto_check = [
     1,     2,     2,     3,   nil,     1 ]

racc_goto_pointer = [
   nil,     0,    -6,    -6 ]

racc_goto_default = [
   nil,   nil,     2,     3 ]

racc_reduce_table = [
  0, 0, :racc_error,
  3, 9, :_reduce_1,
  3, 9, :_reduce_2,
  1, 9, :_reduce_none,
  3, 10, :_reduce_4,
  1, 10, :_reduce_none,
  1, 11, :_reduce_none,
  3, 11, :_reduce_7 ]

racc_reduce_n = 8

racc_shift_n = 16

racc_token_table = {
  false => 0,
  :error => 1,
  "+" => 2,
  "-" => 3,
  "*" => 4,
  :NUMBER => 5,
  "(" => 6,
  ")" => 7 }

racc_nt_base = 8

racc_use_result_var = true

Racc_arg = [
  racc_action_table,
  racc_action_check,
  racc_action_default,
  racc_action_pointer,
  racc_goto_table,
  racc_goto_check,
  racc_goto_default,
  racc_goto_pointer,
  racc_nt_base,
  racc_reduce_table,
  racc_token_table,
  racc_shift_n,
  racc_reduce_n,
  racc_use_result_var ]
Ractor.make_shareable(Racc_arg) if defined?(Ractor)

Racc_token_to_s_table = [
  "$end",
  "error",
  "\"+\"",
  "\"-\"",
  "\"*\"",
  "NUMBER",
  "\"(\"",
  "\")\"",
  "$start",
  "expr",
  "term",
  "factor" ]
Ractor.make_shareable(Racc_token_to_s_table) if defined?(Ractor)

Racc_debug_parser = false

##### State transition tables end #####

# reduce 0 omitted

module_eval(<<'.,.,', 'calc.y', 2)
  def _reduce_1(val, _values, result)
     result = val[0] + val[2]
    result
  end
.,.,

module_eval(<<'.,.,', 'calc.y', 3)
  def _reduce_2(val, _values, result)
     result = val[0] - val[2]
    result
  end
.,.,

# reduce 3 omitted

module_eval(<<'.,.,', 'calc.y', 5)
  def _reduce_4(val, _values, result)
     result = val[0] * val[2]
    result
  end
.,.,

# reduce 5 omitted

# reduce 6 omitted

module_eval(<<'.,.,', 'calc.y', 8)
  def _reduce_7(val, _values, result)
     result = val[1]
    result
  end
.,.,

def _reduce_none(val, _values, result)
  val[0]
end

end   # class TinyCalc

calc = TinyCalc.new
p calc.parse([[:NUMBER, 2], ["+", "+"], [:NUMBER, 3], ["*", "*"], [:NUMBER, 4], [false, false]])
p calc.parse([[:NUMBER, 7], ["-", "-"], [:NUMBER, 2], [false, false]])
p calc.parse([["(", "("], [:NUMBER, 2], ["+", "+"], [:NUMBER, 3], [")", ")"], ["*", "*"], [:NUMBER, 4], [false, false]])
begin
  calc.parse([["+", "+"], [false, false]])
rescue Racc::ParseError => e
  p e.class
end
__END__
14
5
20
Racc::ParseError
