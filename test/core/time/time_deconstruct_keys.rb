t = Time.utc(2026, 7, 16, 13, 45, 30)
p t.deconstruct_keys([:year, :month, :day])
p t.deconstruct_keys([:hour, :min, :sec])
__END__
{year: 2026, month: 7, day: 16}
{hour: 13, min: 45, sec: 30}
