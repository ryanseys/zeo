# A Struct/Data member whose name isn't a plain identifier prints with a
# leading colon in inspect (`:verbose?=`), while a plain one stays bare.

S = Struct.new(:verbose?, :name)
p S.new(true, "x").inspect
D = Data.define(:ok?, :count)
p D.new(ok?: false, count: 3).inspect
__END__
"#<struct S :verbose?=true, name=\"x\">"
"#<data D :ok?=false, count=3>"
