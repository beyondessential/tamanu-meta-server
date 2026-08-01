import { describe, expect, it } from "vitest";
import { humanSeconds } from "./humanDuration";

// Rounding each unit from the previous *rounded* unit compounded the error,
// so a duration could be reported nearly a whole unit longer than it was:
// 1h29m35s became "2h". Each unit is now derived from the raw seconds.
describe("humanSeconds does not compound rounding across units", () => {
	it("does not round a not-quite-90-minute span up to two hours", () => {
		expect(humanSeconds(5375)).toBe("1h"); // 1h29m35s
	});

	it("does not round a not-quite-36-hour span up to two days", () => {
		expect(humanSeconds(129_480)).toBe("1d"); // 1d11h58m
	});

	it("still rounds honestly when the duration really is near the mark", () => {
		expect(humanSeconds(5_400)).toBe("2h"); // 1h30m — rounds up on its own
		expect(humanSeconds(138_240)).toBe("2d"); // 1d14h24m
	});

	it("steps up a unit rather than reporting 60 of the smaller one", () => {
		expect(humanSeconds(3_599)).toBe("1h");
		expect(humanSeconds(86_399)).toBe("1d");
	});

	it("keeps the simple cases", () => {
		expect(humanSeconds(0)).toBe("0s");
		expect(humanSeconds(-5)).toBe("0s");
		expect(humanSeconds(1)).toBe("1s");
		expect(humanSeconds(59)).toBe("59s");
		expect(humanSeconds(60)).toBe("1m");
		expect(humanSeconds(3_600)).toBe("1h");
		expect(humanSeconds(86_400)).toBe("1d");
	});
});
