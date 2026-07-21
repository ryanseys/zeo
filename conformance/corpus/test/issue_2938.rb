# begin/end on a Range read out of a poly container: the poly receiver reads
# the sp_Range endpoints instead of returning an opaque value.
ranges = [(1..5), (10..20)]
p ranges[0].begin + ranges[0].end
p ranges[1].begin
p ranges[1].end
