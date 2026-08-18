# An ERB template compiles to a Ruby snippet the eval VM runs, and a block tag
# makes that snippet a block over the snippet's OWN local: every `<%= %>`
# inside `<% 3.times do |i| %>` appends to the `_erbout` the line above it
# bound. A block that got a fresh scope read that name as a method call.
require "erb"

p ERB.new("<% 3.times do |i| %><%= i %><% end %>").result(binding)
p ERB.new("<% [1, 2, 3].each { |i| %>[<%= i * 2 %>]<% } %>").result(binding)
p ERB.new("<% %w[a b].each_with_index do |s, i| %><%= i %>:<%= s %> <% end %>").result(binding)
p ERB.new("<% h = { x: 1, y: 2 } %><% h.each do |k, v| %><%= k %>=<%= v %>;<% end %>").result(binding)
p ERB.new("<%= [1, 2].map { |z| z + 1 }.inspect %>").result(binding)

# A local of the CALLER's binding is in scope inside a block tag too.
n = 4
p ERB.new("<%= n %> items<% if n > 2 %> (many)<% end %>").result(binding)
p ERB.new("<% 2.times do %><%= n %><% end %>").result(binding)

# Nested blocks, and the tags that carry no block at all.
p ERB.new("<% 2.times do |a| %><% 2.times do |b| %><%= a %><%= b %><% end %><% end %>").result(binding)
p ERB.new("plain").result(binding)
p ERB.new("<%# a comment %>ok").result(binding)
