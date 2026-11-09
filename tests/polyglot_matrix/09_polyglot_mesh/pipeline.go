package pipeline

func DispatchStep(stage_id int, step_offset int) int {
    return stage_id * 10 + step_offset
}
