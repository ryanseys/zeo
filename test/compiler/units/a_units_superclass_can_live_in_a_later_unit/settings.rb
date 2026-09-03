# The unrelated namespace whose runtime-minted `Path` used to capture
# every `class X < Path` in the program.
module Settings
  Path = Struct.new(:explicit, :system) do
    def kind = "struct"
  end
end
