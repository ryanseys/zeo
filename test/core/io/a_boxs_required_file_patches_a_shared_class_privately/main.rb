box = Ruby::Box.new
box.require_relative "patch"
p Array.respond_to?(:zzz)
p box::Owned.go
