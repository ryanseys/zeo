require "bundler"

p Bundler::CLI.commands.keys.size >= 20
p Bundler::CLI.commands.key?("install")
