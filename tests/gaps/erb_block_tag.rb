require "erb"

p ERB.new("<% 3.times do |i| %><%= i %><% end %>").result(binding)
