require_relative '../../spec_helper'

describe "ObjectSpace.rb_mmtk_roots" do
  it "doesn't crash" do
    ObjectSpace.rb_mmtk_roots.first.should be_an_instance_of(Ractor)
  end
end

