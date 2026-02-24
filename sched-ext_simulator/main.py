from scx_sim import KernelSimulator
from policy import MyPolicy

if __name__ == "__main__":
    simulator = KernelSimulator(num_cpus=4)
    policy = MyPolicy()
    
    # 2 applications which run forever. Config param: number of threads.
    simulator.add_workload("critical", num_threads=2)
    simulator.add_workload("hog", num_threads=4)
    
    simulator.attach_policy(policy)
    simulator.run(duration_ticks=5_000_000) # Run for 5 seconds
    simulator.export_perfetto("trace.json")
    print("Simulation completed successfully. Trace saved to trace.json")
