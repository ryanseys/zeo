[["garbage", "can't parse"], ["2021-12-25", "no time information"]].each do |s, _|
  begin
    Time.new(s)
  rescue ArgumentError => e
    puts "#{e.class}: #{e.message}"
  end
end
__END__
ArgumentError: can't parse: "garbage"
ArgumentError: no time information
