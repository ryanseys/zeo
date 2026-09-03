class TopRisk
  def risky_top
    raise "bad"
  end
end
x = TopRisk.new.risky_top rescue "fallback"
puts x
__END__
fallback
