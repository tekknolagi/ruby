describe :thread_to_s, shared: true do
  sep = ruby_version_is("2.7") ? " " : "@"

  describe "for a thread created with Thread.new" do
    it "returns a description including file and line number" do
      Thread.new { "hello" }.send(@method).should =~ /^#<Thread:([^ ]*?)#{sep}#{Regexp.escape __FILE__}:#{__LINE__ } \w+>$/
    end

    it "has a binary encoding" do
      Thread.new { "hello" }.send(@method).encoding.should == Encoding::BINARY
    end
  end
end
