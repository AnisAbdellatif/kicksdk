defmodule Kicksdk.NativeTest do
  # Changes the environment, so not async.
  use ExUnit.Case, async: false

  test "a change to the build variables recompiles the NIF module" do
    refute Kicksdk.Native.__mix_recompile__?()

    for var <- ~w(KICKSDK_BUILD KICKSDK_BASE_URL TARGET_ARCH) do
      before = System.get_env(var)

      try do
        System.put_env(var, "#{before}changed")
        assert Kicksdk.Native.__mix_recompile__?()
      after
        if before, do: System.put_env(var, before), else: System.delete_env(var)
      end
    end

    refute Kicksdk.Native.__mix_recompile__?()
  end
end
