# `map { |c| rect(**c) }` passes each hash as keyword arguments.
# (spinel issue #2885)
def rect(width:, height:) = width * height
configs = [{ width: 2, height: 3 }, { width: 5, height: 5 }]
p configs.map { |c| rect(**c) }
__END__
[6, 25]
