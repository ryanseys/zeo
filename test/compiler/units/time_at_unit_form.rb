# Time.at(sec, num, :unit) scales the second argument by the named unit
# (#2714). CRuby's unit set has no plurals; an unknown one is ArgumentError.
p Time.at(0, 500, :millisecond).to_f
p Time.at(0, 500, :usec).to_f
p Time.at(0, 500, :microsecond).to_f
p Time.at(0, 500, :nanosecond).to_f
p Time.at(0, 500, :nsec).to_f
p Time.at(0, 1.5, :millisecond).to_f
p Time.at(1, 250, :millisecond).to_f
r = (begin; Time.at(1, 250, :milliseconds); rescue ArgumentError => e; e.message; end)
p r
__END__
0.5
0.0005
0.0005
5.0e-07
5.0e-07
0.0015
1.25
"unexpected unit: milliseconds"
