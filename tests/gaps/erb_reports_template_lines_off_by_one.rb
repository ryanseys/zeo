# An error raised inside an ERB template names the TEMPLATE line: a
# raise on template line 3 reports "(erb):3" (or "tpl.erb:3" with a
# filename). zeo reports one line further (the generated code's
# "#coding:UTF-8" preamble line is being counted). (Found by the
# 2026-08-24 probe sweep.)
require "erb"
t = ERB.new("l1\nl2\n<% raise 'boom' %>\nl4")
begin
  t.result
rescue RuntimeError => e
  p e.backtrace.first[/\(erb\):\d+/]
end
t2 = ERB.new("<% raise 'b' %>")
t2.filename = "tpl.erb"
begin
  t2.result
rescue RuntimeError => e
  p e.backtrace.first[/tpl\.erb:\d+/]
end
