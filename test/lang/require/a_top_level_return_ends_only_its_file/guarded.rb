puts "guarded: start"
if STOPS_MARK
  begin
    return
  rescue StandardError
    puts "guarded: rescue"
  end
end
puts "guarded: never"
